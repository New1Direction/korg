# Korg stream-json Adapter Mapping Table
#
# This adapter transforms Claude Code's `--output-format stream-json --verbose`
# stdin stream into Korg v1 AgentToolCall JSON events and POSTs them to the
# Korg Ingestion API (POST /api/agent/tool-call).
#
# Mapping Rules:
#
# 1. system event with subtype: "init"
#    -> No AgentToolCall emitted.
#    -> Extract session_id, claude_code_version, cwd, model, and permissionMode to store as adapter state.
#    -> Use version for source_agent: "agent:claude-code@<version>".
#    -> If a second system init event occurs mid-stream: Log a WARNING to stderr, discard it,
#       and continue with original session metadata.
#
# 2. user event with content as plain text or content array without tool_use_id
#    -> Emit user_prompt AgentToolCall.
#    -> Set triggered_by = null (root event per spec §7.6).
#    -> Note on v1.1 limitation: For non-interactive sessions started via the CLI (e.g. `claude -p "prompt"`),
#       Claude Code does not emit a "user" stream event for the initial prompt. To preserve the causal
#       walk root invariant, the adapter synthesizes a root "user_prompt" event upon system init.
#       This is a known limitation of v1.1 passive stream auditing; in v1.2, the adapter will capture the
#       original prompt directly from the CLI invocation context.
#
# 3. user event with content array containing a block with tool_use_id
#    -> This represents a tool_result.
#    -> Pair with the buffered tool_use from pending_tool_uses[tool_use_id].
#    -> Emit one AgentToolCall with:
#       - args: from the buffered tool_use.input
#       - result: direct string of the tool result content (Option 1: Always use
#         user.message.content[].content as the single source of truth for all tools,
#         preventing duplication of tool_use_result fields and remaining general).
#       - If the result string is >1KB, the string value is replaced directly by the sentinel
#         {"_ref": "sha256:<digest>", "size_bytes": N} without wrapping (spec §3).
#       - tool_name: from the buffered tool_use.name
#       - triggered_by: last_llm_seq (the llm_inference that returned it)
#       - success: not is_error
#       - duration_ms: derived via wall-clock time.monotonic() delta from buffer-time
#    -> Per spec §1, single event per completed call.
#
# 4. assistant event with content array containing tool_use blocks
#    -> For each tool_use:
#       - Buffer in pending_tool_uses[tool_use.id], recording time.monotonic() start time.
#       - Do NOT emit yet.
#    -> Emit exactly one llm_inference AgentToolCall for the assistant turn itself:
#       - triggered_by: last_user_or_result_seq (most recent user_prompt or tool_result seq_id)
#
# 5. assistant event with no tool_use blocks (text-only response)
#    -> Emit exactly one llm_inference AgentToolCall.
#    -> triggered_by: last_user_or_result_seq.
#    -> No buffered tools.
#
# 6. result event (terminal)
#    -> Emit a session_complete AgentToolCall event.
#    -> triggered_by: last_emitted_seq.
#    -> args: Environment metadata stored in adapter state:
#       {"cwd": cwd, "model": model, "permission_mode": permission_mode}
#    -> result (Strict Allowlist):
#       - subtype (success/failure)
#       - duration_ms (session runtime)
#       - total_cost_usd (financial cost)
#       - summary (value of the 'result' string field)
#       - terminal_reason
#       - stop_reason
#       - permission_denials
#       - num_turns
#    -> Pruned/Dropped fields: usage, modelUsage, ttft_ms, api_error_status.
#
# 7. Fallback for Stream Starting Without system init
#    -> Log a loud WARNING at startup on stderr:
#       "stream began without system init event, source_agent defaulting to claude-code@unknown, this may indicate a format change or upstream error."
#    -> Default source_agent = "agent:claude-code@unknown".
#    -> Generate a random session ID and proceed without crashing.
#
# 8. Unmapped/Unknown top-level event types
#    -> Any top-level 'type' value not in {system, assistant, user, result}
#       is written to unknown_events.log and no event is emitted.
#
# Causality State:
# - last_emitted_seq: seq_id returned by Korg for the most recent POST
# - last_user_or_result_seq: seq_id of the most recent user_prompt or tool_result
# - last_llm_seq: seq_id of the most recent llm_inference
# - pending_tool_uses: dict keyed by tool_use_id holding (tool_use, start_time)

import sys
import json
import time
import uuid
import hashlib
import os
import threading
import queue

# Standard dependency requests is used to interact with Korg HTTP Ingestion API
try:
    import requests
except ImportError:
    # Standard fallback so users know what to install
    sys.stderr.write("ERROR: The 'requests' library is required. Install it using: pip install requests\n")
    sys.exit(1)

# Global Causality State
last_emitted_seq = None
pending_tool_uses = {}

# Local Sequence Logic to eliminate race conditions
local_seq_counter = 0
local_to_server_seq = {}
last_local_user_or_result_seq = None
last_local_llm_seq = None
last_local_emitted_seq = None

# Session Metadata State
session_id = None
claude_code_version = None
cwd = None
model = None
permission_mode = None
source_agent = "agent:claude-code@unknown"
init_received = False

# Queue & Background Writer Setup (Preserving §7.5 causality)
event_queue = queue.Queue(maxsize=256)
queue_lock = threading.Lock()
korg_base_url = os.environ.get("KORG_BASE_URL", "http://localhost:8080")

def korg_writer_worker():
    """Background worker draining the queue and issuing serial POST requests."""
    global last_emitted_seq
    while True:
        event = event_queue.get()
        if event is None:
            event_queue.task_done()
            break
        
        # Pull parsing-side local IDs
        local_seq_id = event.pop("local_seq_id", None)
        local_triggered_by = event.pop("local_triggered_by", None)
        
        # Translate local_triggered_by to the server's seq_id
        if local_triggered_by is not None:
            event["triggered_by"] = local_to_server_seq.get(local_triggered_by)
        else:
            event["triggered_by"] = None
        
        # Issue synchronous serial POST request
        url = f"{korg_base_url.rstrip('/')}/api/agent/tool-call"
        try:
            headers = {"Content-Type": "application/json"}
            res = requests.post(url, json=event, headers=headers, timeout=5.0)
            if res.status_code == 200:
                data = res.json()
                seq_id = data.get("seq_id")
                if seq_id is not None:
                    last_emitted_seq = seq_id
                    if local_seq_id is not None:
                        local_to_server_seq[local_seq_id] = seq_id
            else:
                sys.stderr.write(f"WARNING: Korg server returned {res.status_code}: {res.text}\n")
        except Exception as e:
            sys.stderr.write(f"WARNING: Failed to POST event to Korg: {e}\n")
        
        event_queue.task_done()

# Start background worker thread
writer_thread = threading.Thread(target=korg_writer_worker, name="korg-stream-writer", daemon=True)
writer_thread.start()

def log_unknown_event(event):
    """Logs raw unrecognized events to unknown_events.log for analysis."""
    try:
        with open("unknown_events.log", "a", encoding="utf-8") as f:
            f.write(json.dumps(event) + "\n")
    except Exception as e:
        sys.stderr.write(f"ERROR: Failed to write to unknown_events.log: {e}\n")

def canonical_hash_and_store_blob(value, project_cwd):
    """
    Computes deterministic SHA-256 digest and writes to blob store.
    Conforms to spec §3 and §7.2 (alphabetical sorted keys, UTF-8 encoded).
    """
    if isinstance(value, (dict, list)):
        serialized = json.dumps(value, separators=(',', ':'), sort_keys=True)
        raw_bytes = serialized.encode("utf-8")
    elif isinstance(value, str):
        raw_bytes = value.encode("utf-8")
    elif isinstance(value, bytes):
        raw_bytes = value
    else:
        serialized = str(value)
        raw_bytes = serialized.encode("utf-8")
        
    sha256 = hashlib.sha256(raw_bytes).hexdigest()
    size_bytes = len(raw_bytes)
    
    # Save physically to the workspace .korg/blobs/
    base_dir = project_cwd if project_cwd else os.getcwd()
    blob_dir = os.path.join(base_dir, ".korg", "blobs", sha256[:2])
    blob_path = os.path.join(blob_dir, sha256)
    
    try:
        os.makedirs(blob_dir, exist_ok=True)
        with open(blob_path, "wb") as f:
            f.write(raw_bytes)
    except Exception as e:
        sys.stderr.write(f"ERROR: Failed to write content-addressed blob {sha256}: {e}\n")
        
    return sha256, size_bytes

def process_content_refs(obj, project_cwd):
    """
    Recursively scans arguments or results and replaces any field values
    larger than 1024 bytes with a content ref sentinel.
    """
    if isinstance(obj, dict):
        new_dict = {}
        for k, v in obj.items():
            if isinstance(v, (dict, list)):
                # If the nested structure serialized exceeds 1KB, chunk it
                serialized = json.dumps(v, separators=(',', ':'), sort_keys=True)
                if len(serialized.encode("utf-8")) > 1024:
                    digest, size = canonical_hash_and_store_blob(v, project_cwd)
                    new_dict[k] = {"_ref": f"sha256:{digest}", "size_bytes": size}
                else:
                    new_dict[k] = process_content_refs(v, project_cwd)
            elif isinstance(v, str) and len(v.encode("utf-8")) > 1024:
                digest, size = canonical_hash_and_store_blob(v, project_cwd)
                new_dict[k] = {"_ref": f"sha256:{digest}", "size_bytes": size}
            elif isinstance(v, bytes) and len(v) > 1024:
                digest, size = canonical_hash_and_store_blob(v, project_cwd)
                new_dict[k] = {"_ref": f"sha256:{digest}", "size_bytes": size}
            else:
                new_dict[k] = v
        return new_dict
    elif isinstance(obj, list):
        return [process_content_refs(item, project_cwd) for item in obj]
    else:
        return obj

def enqueue_korg_event(mapped_event):
    """
    Locks the queue and puts the event, executing drop-oldest if full.
    Tracks and sets local sequence IDs and causal triggered_by links synchronously.
    """
    global local_seq_counter, last_local_emitted_seq, last_local_user_or_result_seq, last_local_llm_seq
    
    # Increment local sequence counter and stamp local keys
    local_seq_counter += 1
    local_seq_id = local_seq_counter
    mapped_event["local_seq_id"] = local_seq_id
    
    tool_name = mapped_event.get("tool_name")
    
    # Establish causal triggered_by links locally
    if tool_name == "user_prompt":
        mapped_event["local_triggered_by"] = None
    elif tool_name == "llm_inference":
        mapped_event["local_triggered_by"] = last_local_user_or_result_seq
    elif tool_name == "session_complete":
        mapped_event["local_triggered_by"] = last_local_emitted_seq
    else:
        # A standard tool_call (Read, Write, Edit, Bash) is triggered by its LLM round
        mapped_event["local_triggered_by"] = last_local_llm_seq

    # Update main-thread local causality trackers instantly
    if tool_name == "user_prompt" or tool_name not in ["llm_inference", "session_complete"]:
        last_local_user_or_result_seq = local_seq_id
    elif tool_name == "llm_inference":
        last_local_llm_seq = local_seq_id
        
    last_local_emitted_seq = local_seq_id

    # Run the content-ref chunking processor on both args and result fields
    if "args" in mapped_event:
        mapped_event["args"] = process_content_refs(mapped_event["args"], cwd)
    if "result" in mapped_event:
        # If the result field itself is a string > 1KB, process_content_refs won't chunk the top-level
        # directly because it only iterates lists and dicts. Let's handle top-level string/bytes result chunking:
        result_val = mapped_event["result"]
        if isinstance(result_val, str) and len(result_val.encode("utf-8")) > 1024:
            digest, size = canonical_hash_and_store_blob(result_val, cwd)
            mapped_event["result"] = {"_ref": f"sha256:{digest}", "size_bytes": size}
        elif isinstance(result_val, bytes) and len(result_val) > 1024:
            digest, size = canonical_hash_and_store_blob(result_val, cwd)
            mapped_event["result"] = {"_ref": f"sha256:{digest}", "size_bytes": size}
        else:
            mapped_event["result"] = process_content_refs(result_val, cwd)
        
    # Populate payload_refs with all ContentRef dicts referenced in content refs
    payload_refs = []
    def extract_refs(val, key_label=""):
        if isinstance(val, dict):
            if "_ref" in val and isinstance(val["_ref"], str) and val["_ref"].startswith("sha256:"):
                digest = val["_ref"].replace("sha256:", "")
                payload_refs.append({
                    "sha256": digest,
                    "size_bytes": val.get("size_bytes", 0),
                    "label": key_label or tool_name or "payload"
                })
            else:
                for k, v in val.items():
                    extract_refs(v, k)
        elif isinstance(val, list):
            for item in val:
                extract_refs(item, key_label)
                
    extract_refs(mapped_event.get("args"))
    extract_refs(mapped_event.get("result"))
    mapped_event["payload_refs"] = payload_refs
        
    with queue_lock:
        if event_queue.full():
            try:
                event_queue.get_nowait()
                sys.stderr.write("WARNING: Queue full, dropping oldest event\n")
            except queue.Empty:
                pass
        event_queue.put_nowait(mapped_event)

def parse_stream_line(line):
    """Parses a single JSON line and executes the mapped event transforms."""
    global init_received, session_id, claude_code_version, cwd, model, permission_mode, source_agent
    
    if not line.strip():
        return
        
    try:
        event = json.loads(line)
    except json.JSONDecodeError:
        # Invalid JSON is skipped/logged
        sys.stderr.write("WARNING: Failed to parse JSON line from stream\n")
        return
        
    event_type = event.get("type")
    
    # 8. Unmapped/Unknown top-level event types
    if event_type not in ["system", "assistant", "user", "result"]:
        log_unknown_event(event)
        return
        
    # 7. Fallback for Stream Starting Without system init
    if not init_received and event_type != "system":
        init_received = True
        session_id = str(uuid.uuid4())
        source_agent = "agent:claude-code@unknown"
        sys.stderr.write(
            "WARNING: stream began without system init event, "
            "source_agent defaulting to claude-code@unknown, this may indicate a format change or upstream error.\n"
        )
        # Synthesize user prompt root event for fallback starting
        # Note: Synthesized root prompt is a known v1.1 limitation for stream-only capture of non-interactive CLI sessions.
        mapped_prompt = {
            "source_agent": "human:claude-code-user",
            "tool_name": "user_prompt",
            "args": {"prompt": f"Claude Code session {session_id} initialized without system init (passive fallback)"},
            "result": "Success",
            "payload_refs": [],
            "success": True,
            "duration_ms": 0
        }
        enqueue_korg_event(mapped_prompt)
        
    if event_type == "system":
        subtype = event.get("subtype")
        if subtype == "init":
            if init_received:
                # Mid-stream second system init
                sys.stderr.write(
                    "WARNING: Second system init event occurred mid-stream. "
                    "Discarding it to preserve original session metadata.\n"
                )
                return
            
            init_received = True
            session_id = event.get("session_id", str(uuid.uuid4()))
            claude_code_version = event.get("claude_code_version")
            cwd = event.get("cwd")
            model = event.get("model")
            permission_mode = event.get("permissionMode")
            
            if claude_code_version:
                source_agent = f"agent:claude-code@{claude_code_version}"
            else:
                source_agent = "agent:claude-code@unknown"
                
            # Synthesize user prompt root event for normal initialization
            # Note: Synthesized root prompt is a known v1.1 limitation for stream-only capture of non-interactive CLI sessions.
            mapped_prompt = {
                "source_agent": "human:claude-code-user",
                "tool_name": "user_prompt",
                "args": {"prompt": f"Claude Code session {session_id} initialized via stream-json adapter"},
                "result": "Success",
                "payload_refs": [],
                "success": True,
                "duration_ms": 0
            }
            enqueue_korg_event(mapped_prompt)
                
    elif event_type == "user":
        message = event.get("message", {})
        content = message.get("content", [])
        
        # Check if this content is a list containing a tool_result block
        tool_result_block = None
        if isinstance(content, list):
            for block in content:
                if isinstance(block, dict) and block.get("type") == "tool_result":
                    tool_result_block = block
                    break
                    
        if tool_result_block:
            # 3. Pair buffered tool_use with this tool_result
            tool_use_id = tool_result_block.get("tool_use_id")
            if tool_use_id in pending_tool_uses:
                tool_use, start_time = pending_tool_uses.pop(tool_use_id)
                duration_monotonic = time.monotonic() - start_time
                duration_ms = int(duration_monotonic * 1000)
                
                # Option 1: Always use user.message.content[].content as tool result string
                result_content = tool_result_block.get("content", "")
                
                # Emit completed AgentToolCall event
                mapped = {
                    "source_agent": source_agent,
                    "tool_name": tool_use.get("name"),
                    "args": tool_use.get("input", {}),
                    "result": result_content, # direct string, replaced directly with sentinel if >1KB
                    "payload_refs": [],
                    "success": not tool_result_block.get("is_error", False),
                    "duration_ms": duration_ms
                }
                enqueue_korg_event(mapped)
        else:
            # 2. Plain text user event -> user_prompt
            prompt_text = ""
            if isinstance(content, str):
                prompt_text = content
            elif isinstance(content, list):
                # Extract text blocks
                prompt_text = "\n".join([
                    block.get("text", "") for block in content 
                    if isinstance(block, dict) and block.get("type") == "text"
                ])
                
            mapped = {
                "source_agent": "human:claude-code-user",
                "tool_name": "user_prompt",
                "args": {"prompt": prompt_text},
                "result": {"success": True},
                "payload_refs": [],
                "success": True,
                "duration_ms": 0
            }
            enqueue_korg_event(mapped)
            
    elif event_type == "assistant":
        message = event.get("message", {})
        content = message.get("content", [])
        
        tool_uses = []
        thinking_texts = []
        if isinstance(content, list):
            for block in content:
                if isinstance(block, dict):
                    if block.get("type") == "tool_use":
                        tool_uses.append(block)
                    elif block.get("type") == "thinking":
                        thinking_texts.append(block.get("thinking", ""))
                        
        thinking_summary = "\n".join(thinking_texts)
        
        # 4 & 5. Emit llm_inference for the assistant turn itself
        mapped_inference = {
            "source_agent": source_agent,
            "tool_name": "llm_inference",
            "args": {"thinking": thinking_summary} if thinking_summary else {},
            "result": {"success": True},
            "payload_refs": [],
            "success": True,
            "duration_ms": 0
        }
        enqueue_korg_event(mapped_inference)
        
        # Buffer tool uses for pairing on completion
        for tool_use in tool_uses:
            tool_name = tool_use.get("name")
            # Filter standard handling tools. If unknown, log and skip emission.
            if tool_name in ["Read", "Write", "Edit", "Bash"]:
                pending_tool_uses[tool_use.get("id")] = (tool_use, time.monotonic())
            else:
                log_unknown_event(event)
                
    elif event_type == "result":
        # 6. Terminal result event -> session_complete
        # Strict Allowlist parsing
        subtype = event.get("subtype")
        duration_ms = event.get("duration_ms", 0)
        total_cost_usd = event.get("total_cost_usd", 0.0)
        summary = event.get("result", "")
        terminal_reason = event.get("terminal_reason")
        stop_reason = event.get("stop_reason")
        permission_denials = event.get("permission_denials", [])
        
        usage = event.get("usage", {})
        num_turns = usage.get("num_turns", event.get("num_turns", 0))
        
        mapped_complete = {
            "source_agent": source_agent,
            "tool_name": "session_complete",
            "args": {
                "cwd": cwd,
                "model": model,
                "permission_mode": permission_mode
            },
            "result": {
                "subtype": subtype,
                "duration_ms": duration_ms,
                "total_cost_usd": total_cost_usd,
                "summary": summary,
                "terminal_reason": terminal_reason,
                "stop_reason": stop_reason,
                "permission_denials": permission_denials,
                "num_turns": num_turns
            },
            "payload_refs": [],
            "success": not event.get("is_error", False),
            "duration_ms": duration_ms
        }
        enqueue_korg_event(mapped_complete)

def main():
    """Main stdin stream parsing loop."""
    try:
        for line in sys.stdin:
            parse_stream_line(line)
    except KeyboardInterrupt:
        pass
    finally:
        # Wait for any pending events in the queue to be flushed cleanly
        event_queue.join()
        # Enqueue sentinel to shutdown the poster thread cleanly
        event_queue.put(None)

if __name__ == "__main__":
    main()
