# Korg stream-json Adapter Mapping Table (v1.2)
#
# This adapter transforms Claude Code's `--output-format stream-json --verbose`
# stdin stream into Korg v1 AgentToolCall JSON events and POSTs them to the
# Korg Ingestion API (POST /api/agent/tool-call).
#
# Mapping Rules:
#
# 1. system event with subtype: "init"
#    -> No AgentToolCall emitted.
#    -> Extract session_id, claude_code_version, cwd, model, permissionMode, and tools list.
#    -> tools list populates known_tools set. "Agent" (wire name) is recognized as "Task" (display
#       name) — the only known wire/display mismatch as of Claude Code 2.1.150.
#    -> source_agent for main spine: "agent:claude-code/main@<version>".
#    -> If a second system init event occurs mid-stream: Log WARNING to stderr, discard,
#       and continue with original session metadata.
#
# 2. user event, parent_tool_use_id=null, plain text or content array without tool_use_id
#    -> Emit user_prompt AgentToolCall (main spine).
#    -> Set triggered_by = null (root event per spec §7.6).
#    -> Note on v1.1/v1.2 limitation: For non-interactive sessions started via the CLI (e.g.
#       `claude -p "prompt"`), Claude Code does not emit a "user" stream event for the initial
#       prompt. To preserve the causal walk root invariant, the adapter synthesizes a root
#       "user_prompt" event upon system init. v1.2 captures the CLI prompt directly from
#       the invocation context.
#
# 2b. user event, parent_tool_use_id non-null, plain text (VALIDATED 2026-05-25)
#    -> Sub-agent user_prompt.
#    -> source_agent: "agent:claude-code/sub-{parent_tool_use_id[:8]}@{version}".
#    -> triggered_by: main spine last_llm_seq (cross-spine causal link per spec §2b).
#    -> Initiates a per-ptuid causality bucket in sub_agent_state.
#
# 3. user event, parent_tool_use_id=null, content array with tool_use_id
#    -> Main-spine tool_result: pair with buffered tool_use, emit AgentToolCall.
#    -> Option 1: use user.message.content[].content as the result string — single source of
#       truth for all tools, prevents duplication of tool_use_result fields.
#    -> If result >1KB: replace with content-ref sentinel (spec §3).
#    -> triggered_by: main spine last_llm_seq.
#
# 3b. user event, parent_tool_use_id non-null, content array with tool_use_id
#    -> Sub-agent tool_result: pair with buffered tool_use, emit to sub-agent causality spine.
#
# 4. assistant event, parent_tool_use_id=null
#    -> Emit exactly one llm_inference AgentToolCall (main spine).
#    -> Buffer each tool_use in pending_tool_uses keyed by tool_use_id.
#    -> Deduplication by message.id: streaming deltas for the same message arrive as multiple
#       assistant events with the same message.id. Only emit llm_inference on the first;
#       subsequent deltas only buffer new tool_uses (if any).
#    -> "Agent" (wire name for Task tool) is recognized and buffered like any standard tool.
#
# 4b. assistant event, parent_tool_use_id non-null
#    -> Emit sub-agent llm_inference. Buffer tool_uses on sub-agent spine.
#    -> source_agent: "agent:claude-code/sub-{parent_tool_use_id[:8]}@{version}".
#
# 5. assistant event, no tool_use blocks (text-only response)
#    -> Emit exactly one llm_inference AgentToolCall.
#    -> triggered_by: last_user_or_result_seq on the appropriate spine.
#    -> No buffered tools.
#
# 6. result event (terminal)
#    -> Emit a session_complete AgentToolCall event.
#    -> triggered_by: main spine last_emitted_seq.
#    -> args: environment metadata (cwd, model, permission_mode).
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
# 7. Fallback for stream starting without system init
#    -> Log WARNING to stderr.
#    -> source_agent = "agent:claude-code/main@unknown".
#    -> Generate a random session ID and proceed without crashing.
#
# 8. Unmapped/Unknown top-level event types
#    -> Written to unknown_events.log. No event emitted.
#    -> rate_limit_event: silently dropped (intentionally-not audited — rate-limit noise).
#    -> system subtype hook_started/hook_response: silently dropped (hook lifecycle, not agent behavior).
#    -> system subtype task_started/task_notification: silently dropped (task lifecycle, redundant).
#    -> system subtype task_progress: silently dropped (v1.3 may emit ProgressUpdate events).
#    -> Unknown system subtypes: written to unknown_events.log.
#
# Causality State:
# - Main spine globals: last_local_user_or_result_seq, last_local_llm_seq, last_local_emitted_seq
# - Sub-agent spine: sub_agent_state[ptuid] = {last_user_or_result_seq, last_llm_seq, last_emitted_seq}
# - pending_tool_uses: global {tool_use_id: (tool_use, start_time)} — shared across all spines
# - seen_assistant_ids: deduplicates streaming assistant events by message.id

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
    sys.stderr.write("ERROR: The 'requests' library is required. Install it using: pip install requests\n")
    sys.exit(1)

# System subtypes that are silently dropped (not audited)
_SILENT_SYSTEM_SUBTYPES = frozenset({
    "hook_started", "hook_response",      # hook lifecycle — not agent behavior
    "task_started", "task_notification",  # task lifecycle — redundant with agent events
    "task_progress",                      # deferred (v1.3 may emit ProgressUpdate)
})
# Top-level event types that are silently dropped
_SILENT_EVENT_TYPES = frozenset({"rate_limit_event"})

# Global Causality State (main spine)
last_emitted_seq = None
pending_tool_uses = {}

# Local Sequence Logic to eliminate race conditions
local_seq_counter = 0
local_to_server_seq = {}
last_local_user_or_result_seq = None
last_local_llm_seq = None
last_local_emitted_seq = None

# Sub-agent causality state — keyed by parent_tool_use_id
sub_agent_state = {}

# Session Metadata State
session_id = None
claude_code_version = None
cwd = None
model = None
permission_mode = None
source_agent = "agent:claude-code/main@unknown"
init_received = False

# Tools recognized from system/init; "Agent" (wire) maps to "Task" (display).
known_tools = set()

# Streaming assistant event deduplication by message.id
seen_assistant_ids = set()

# Queue & Background Writer Setup (Preserving §7.5 causality)
event_queue = queue.Queue(maxsize=256)
queue_lock = threading.Lock()
korg_base_url = os.environ.get("KORG_BASE_URL", "http://localhost:8080")

def korg_writer_worker():
    """Background worker draining the queue and issuing serial POST requests.

    On a 5s timeout / network error / non-2xx response, we retry once with a
    1s pause before giving up. Previously a single transient blip dropped
    the event silently — a single retry catches the common short outage
    without blocking the writer long enough to overflow the queue.
    """
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

        url = f"{korg_base_url.rstrip('/')}/api/agent/tool-call"
        headers = {"Content-Type": "application/json"}
        max_attempts = 2
        retry_delay_secs = 1.0
        for attempt in range(1, max_attempts + 1):
            try:
                res = requests.post(url, json=event, headers=headers, timeout=5.0)
                if res.status_code == 200:
                    data = res.json()
                    seq_id = data.get("seq_id")
                    if seq_id is not None:
                        last_emitted_seq = seq_id
                        if local_seq_id is not None:
                            local_to_server_seq[local_seq_id] = seq_id
                    break  # success
                sys.stderr.write(
                    f"WARNING: Korg server returned {res.status_code} "
                    f"(attempt {attempt}/{max_attempts}): {res.text}\n"
                )
            except Exception as e:
                sys.stderr.write(
                    f"WARNING: Failed to POST event to Korg "
                    f"(attempt {attempt}/{max_attempts}): {e}\n"
                )
            if attempt < max_attempts:
                time.sleep(retry_delay_secs)

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


def log_unknown_tool(tool_name, tool_id):
    """Compact unknown-tool log — name + id only, not the entire assistant event.

    Logging the full assistant event is several KB per occurrence and obscures
    the actually-useful signal (which tool name we didn't recognise).
    """
    try:
        with open("unknown_events.log", "a", encoding="utf-8") as f:
            f.write(
                json.dumps({"unknown_tool": tool_name, "tool_use_id": tool_id})
                + "\n"
            )
    except Exception as e:
        sys.stderr.write(f"ERROR: Failed to write to unknown_events.log: {e}\n")

def _stable_default(obj):
    """json.dumps default-hook that always returns a deterministic string.

    Falling back to str(obj) (the old behaviour) yields '<__main__.Foo object
    at 0x...>' for custom types, which changes every run and breaks the
    spec §3/§7.2 invariant that two agents hashing the same logical content
    produce the same SHA-256. A type-name placeholder is deterministic and
    loud enough that pipelines can flag it.
    """
    return f"<unhashable-type:{type(obj).__name__}>"


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
        try:
            serialized = json.dumps(
                value, separators=(',', ':'), sort_keys=True, default=_stable_default
            )
        except (TypeError, ValueError) as exc:
            sys.stderr.write(
                f"WARNING: blob value of type {type(value).__name__} is not JSON-serialisable "
                f"({exc}); substituting deterministic placeholder.\n"
            )
            serialized = _stable_default(value)
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
    # Defense in depth: if a stream skipped system/init we may be called with
    # project_cwd=None. canonical_hash_and_store_blob already falls back to
    # os.getcwd(), but normalising here keeps the contract self-contained.
    if project_cwd is None:
        project_cwd = os.getcwd()
    if isinstance(obj, dict):
        new_dict = {}
        for k, v in obj.items():
            if isinstance(v, (dict, list)):
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

def enqueue_korg_event(mapped_event, ptuid=None):
    """
    Locks the queue and puts the event, executing drop-oldest if full.
    Tracks and sets local sequence IDs and causal triggered_by links synchronously.
    ptuid: parent_tool_use_id — when non-None, routes causality through the sub-agent spine.
    """
    global local_seq_counter, last_local_emitted_seq, last_local_user_or_result_seq, last_local_llm_seq

    # Increment local sequence counter and stamp local key
    local_seq_counter += 1
    local_seq_id = local_seq_counter
    mapped_event["local_seq_id"] = local_seq_id

    tool_name = mapped_event.get("tool_name")

    if ptuid is not None:
        # Sub-agent spine — per-ptuid causality bucket
        state = sub_agent_state.setdefault(ptuid, {
            "last_user_or_result_seq": None,
            "last_llm_seq": None,
            "last_emitted_seq": None,
        })
        if tool_name == "user_prompt":
            # Cross-spine: sub-agent root triggered by the main-spine llm_inference
            # that contained the spawning Task tool_use. Look it up via the
            # ptuid (= tool_use id). Falling back to last_local_llm_seq is
            # order-fragile — if any other main-spine llm_inference fires
            # between buffering and this user_prompt, the global is stale.
            spawn_entry = pending_tool_uses.get(ptuid)
            spawning_seq = spawn_entry[2] if spawn_entry and len(spawn_entry) >= 3 else None
            if spawning_seq is None:
                # Defensive fallback. Hits when the parent tool_use wasn't
                # buffered (unknown tool, or sub-agent event arrived before
                # the spawning assistant chunk). Better than None — at least
                # it chains to *some* main-spine event.
                spawning_seq = last_local_llm_seq
                if ptuid not in pending_tool_uses:
                    sys.stderr.write(
                        f"WARNING: sub-agent user_prompt with ptuid={ptuid[:12]}... "
                        f"has no buffered parent tool_use; falling back to last_local_llm_seq\n"
                    )
            mapped_event["local_triggered_by"] = spawning_seq
        elif tool_name == "llm_inference":
            mapped_event["local_triggered_by"] = state["last_user_or_result_seq"]
        elif tool_name == "session_complete":
            mapped_event["local_triggered_by"] = state["last_emitted_seq"]
        else:
            mapped_event["local_triggered_by"] = state["last_llm_seq"]

        if tool_name == "user_prompt" or tool_name not in ["llm_inference", "session_complete"]:
            state["last_user_or_result_seq"] = local_seq_id
        elif tool_name == "llm_inference":
            state["last_llm_seq"] = local_seq_id
        state["last_emitted_seq"] = local_seq_id
    else:
        # Main spine
        if tool_name == "user_prompt":
            # Session root → None. A multi-turn follow-up (a prompt after at least
            # one llm_inference) chains to the prior llm_inference, matching the
            # claude-code user_followup causality — not a disconnected root.
            mapped_event["local_triggered_by"] = last_local_llm_seq
        elif tool_name == "llm_inference":
            # Per spec §2a: round-N's llm_inference chains to round-(N-1)'s
            # llm_inference, not the most recent user/tool_result. For round 1
            # there's no prior llm_inference so we fall back to the root
            # user_prompt seq, satisfying the "every event has a root" invariant.
            mapped_event["local_triggered_by"] = (
                last_local_llm_seq
                if last_local_llm_seq is not None
                else last_local_user_or_result_seq
            )
        elif tool_name == "session_complete":
            mapped_event["local_triggered_by"] = last_local_emitted_seq
        else:
            # Standard tool_call triggered by its LLM round
            mapped_event["local_triggered_by"] = last_local_llm_seq

        if tool_name == "user_prompt" or tool_name not in ["llm_inference", "session_complete"]:
            last_local_user_or_result_seq = local_seq_id
        elif tool_name == "llm_inference":
            last_local_llm_seq = local_seq_id
        last_local_emitted_seq = local_seq_id

    # Run the content-ref chunking processor on both args and result fields
    if "args" in mapped_event:
        mapped_event["args"] = process_content_refs(mapped_event["args"], cwd)
    if "result" in mapped_event:
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
                dropped = event_queue.get_nowait()
                dropped_seq = dropped.get("local_seq_id", "?")
                # Surface the dropped seq so consumers reading stderr can
                # reconstruct the causal-chain gap. Previously the warning
                # said nothing about which event was lost.
                sys.stderr.write(
                    f"WARNING: korg-adapter queue full (capacity 256); "
                    f"dropped local_seq={dropped_seq}. Causal chain has a gap here.\n"
                )
            except queue.Empty:
                pass
        event_queue.put_nowait(mapped_event)

def _sub_agent_source(ptuid):
    """Returns source_agent string for a sub-agent identified by ptuid."""
    version = claude_code_version or "unknown"
    return f"agent:claude-code/sub-{ptuid[:8]}@{version}"

def parse_stream_line(line):
    """Parses a single JSON line and executes the mapped event transforms."""
    global init_received, session_id, claude_code_version, cwd, model, permission_mode, source_agent

    if not line.strip():
        return

    try:
        event = json.loads(line)
    except json.JSONDecodeError:
        sys.stderr.write("WARNING: Failed to parse JSON line from stream\n")
        return

    event_type = event.get("type")

    # Silent-drop known-but-unaudited top-level types (e.g. rate_limit_event)
    if event_type in _SILENT_EVENT_TYPES:
        return

    # 8. Unmapped/Unknown top-level event types
    if event_type not in {"system", "assistant", "user", "result"}:
        log_unknown_event(event)
        return

    # 7. Fallback for stream starting without system init
    if not init_received and event_type != "system":
        init_received = True
        session_id = str(uuid.uuid4())
        source_agent = "agent:claude-code/main@unknown"
        sys.stderr.write(
            "WARNING: stream began without system init event, "
            "source_agent defaulting to claude-code/main@unknown, this may indicate a format change or upstream error.\n"
        )
        # Synthesize user prompt root event for fallback starting
        mapped_prompt = {
            "source_agent": "human:claude-code-user",
            "tool_name": "user_prompt",
            "args": {"prompt": "[adapter-synthesized: original CLI prompt not captured in stream-json; see v1.2]"},
            "result": "Success",
            "payload_refs": [],
            "success": True,
            "duration_ms": 0
        }
        enqueue_korg_event(mapped_prompt)

    if event_type == "system":
        subtype = event.get("subtype")

        # Silently drop hook and task lifecycle subtypes
        if subtype in _SILENT_SYSTEM_SUBTYPES:
            return

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

            # Populate recognized tools from init list
            known_tools.clear()
            known_tools.update(event.get("tools", []))

            if claude_code_version:
                source_agent = f"agent:claude-code/main@{claude_code_version}"
            else:
                source_agent = "agent:claude-code/main@unknown"

            # Synthesize user prompt root event
            mapped_prompt = {
                "source_agent": "human:claude-code-user",
                "tool_name": "user_prompt",
                "args": {"prompt": "[adapter-synthesized: original CLI prompt not captured in stream-json; see v1.2]"},
                "result": "Success",
                "payload_refs": [],
                "success": True,
                "duration_ms": 0
            }
            enqueue_korg_event(mapped_prompt)
        else:
            # Unknown system subtype — log for investigation
            log_unknown_event(event)

    elif event_type == "user":
        message = event.get("message", {})
        content = message.get("content", [])
        ptuid = event.get("parent_tool_use_id")

        # Check if this content contains a tool_result block
        tool_result_block = None
        if isinstance(content, list):
            for block in content:
                if isinstance(block, dict) and block.get("type") == "tool_result":
                    tool_result_block = block
                    break

        if tool_result_block:
            # 3/3b. Pair buffered tool_use with this tool_result
            tool_use_id = tool_result_block.get("tool_use_id")
            if tool_use_id in pending_tool_uses:
                entry = pending_tool_uses.pop(tool_use_id)
                tool_use, start_time = entry[0], entry[1]
                # entry[2] is spawning_llm_seq, only consumed by the cross-spine
                # user_prompt path; not needed here for the tool_result emit.
                duration_ms = int((time.monotonic() - start_time) * 1000)

                # Option 1: use user.message.content[].content as result string
                result_content = tool_result_block.get("content", "")

                # Flatten content list to string (Agent/Task tool returns [{type:text, text:...}])
                if isinstance(result_content, list):
                    result_content = "\n".join(
                        b.get("text", "") for b in result_content
                        if isinstance(b, dict) and b.get("type") == "text"
                    )

                agent = _sub_agent_source(ptuid) if ptuid else source_agent
                mapped = {
                    "source_agent": agent,
                    "tool_name": tool_use.get("name"),
                    "args": tool_use.get("input", {}),
                    "result": result_content,
                    "payload_refs": [],
                    "success": not tool_result_block.get("is_error", False),
                    "duration_ms": duration_ms
                }
                enqueue_korg_event(mapped, ptuid=ptuid)
        else:
            # 2/2b. Plain text user event -> user_prompt
            prompt_text = ""
            if isinstance(content, str):
                prompt_text = content
            elif isinstance(content, list):
                prompt_text = "\n".join([
                    block.get("text", "") for block in content
                    if isinstance(block, dict) and block.get("type") == "text"
                ])

            if ptuid:
                # 2b. Sub-agent user_prompt — cross-spine causal link
                mapped = {
                    "source_agent": _sub_agent_source(ptuid),
                    "tool_name": "user_prompt",
                    "args": {"prompt": prompt_text},
                    "result": {"success": True},
                    "payload_refs": [],
                    "success": True,
                    "duration_ms": 0
                }
                enqueue_korg_event(mapped, ptuid=ptuid)
            else:
                # 2. Main-spine user_prompt
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
        ptuid = event.get("parent_tool_use_id")
        msg_id = message.get("id")

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

        # Deduplication: same message.id may arrive as multiple streaming deltas
        is_new_message = msg_id not in seen_assistant_ids
        if msg_id:
            seen_assistant_ids.add(msg_id)

        agent = _sub_agent_source(ptuid) if ptuid else source_agent

        if is_new_message:
            # 4/4b/5. Emit llm_inference for this assistant turn
            mapped_inference = {
                "source_agent": agent,
                "tool_name": "llm_inference",
                "args": {"thinking": thinking_summary} if thinking_summary else {},
                "result": {"success": True},
                "payload_refs": [],
                "success": True,
                "duration_ms": 0
            }
            enqueue_korg_event(mapped_inference, ptuid=ptuid)

        # Buffer tool_uses (may arrive in a later streaming chunk for the same
        # message.id). Capture last_local_llm_seq alongside so a future
        # cross-spine lookup (sub-agent user_prompt with ptuid=this tool_use's
        # id) can resolve the SPAWNING llm_inference seq regardless of how
        # last_local_llm_seq evolved after this buffer call.
        spawning_llm_seq = last_local_llm_seq
        for tool_use in tool_uses:
            tname = tool_use.get("name")
            tid = tool_use.get("id")
            if tid in pending_tool_uses:
                continue  # already buffered from an earlier streaming chunk
            is_known = tname in known_tools or (tname == "Agent" and "Task" in known_tools)
            if is_known:
                pending_tool_uses[tid] = (tool_use, time.monotonic(), spawning_llm_seq)
            else:
                log_unknown_tool(tname, tid)

    elif event_type == "result":
        # 6. Terminal result event -> session_complete
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
