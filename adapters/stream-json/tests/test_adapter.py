import unittest
from unittest.mock import patch, MagicMock
import json
import time
import tempfile
import shutil
import os
import sys

# Append the parent directory to sys.path so we can import the adapter
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "..")))

import korg_stream_adapter as adapter

class TestKorgStreamAdapter(unittest.TestCase):
    def setUp(self):
        # Reset global causality and session state before each test
        adapter.last_emitted_seq = None
        adapter.local_seq_counter = 0
        adapter.local_to_server_seq.clear()
        adapter.last_local_user_or_result_seq = None
        adapter.last_local_llm_seq = None
        adapter.last_local_emitted_seq = None
        adapter.pending_tool_uses.clear()
        adapter.sub_agent_state.clear()
        adapter.known_tools.clear()
        adapter.seen_assistant_ids.clear()

        # By default, pretend init is already received to avoid fallback prompt injection in standard tests
        adapter.init_received = True

        adapter.session_id = None
        adapter.claude_code_version = None
        adapter.cwd = None
        adapter.model = None
        adapter.permission_mode = None
        adapter.source_agent = "agent:claude-code/main@unknown"

        # Clear queue
        while not adapter.event_queue.empty():
            try:
                adapter.event_queue.get_nowait()
                adapter.event_queue.task_done()
            except Exception:
                pass

        # Create temp dir for workspace mock
        self.test_dir = tempfile.mkdtemp()
        adapter.cwd = self.test_dir

        # Clean unknown_events.log if it exists
        if os.path.exists("unknown_events.log"):
            try:
                os.remove("unknown_events.log")
            except Exception:
                pass

    def tearDown(self):
        shutil.rmtree(self.test_dir)
        if os.path.exists("unknown_events.log"):
            try:
                os.remove("unknown_events.log")
            except Exception:
                pass

    @patch("requests.post")
    def test_init_event_extracts_version_and_session_id(self, mock_post):
        # We manually reset init_received to False for testing initialization
        adapter.init_received = False

        mock_res = MagicMock()
        mock_res.status_code = 200
        mock_res.json.return_value = {"seq_id": 1}
        mock_post.return_value = mock_res

        # 1. Parse a system/init event
        init_json = {
            "type": "system",
            "subtype": "init",
            "cwd": self.test_dir,
            "session_id": "test-session-1234",
            "model": "claude-opus-4-7",
            "permissionMode": "auto",
            "claude_code_version": "2.1.150",
            "tools": ["Task", "Bash", "Read", "Write", "Edit"]
        }
        adapter.parse_stream_line(json.dumps(init_json))
        adapter.event_queue.join()

        self.assertTrue(adapter.init_received)
        self.assertEqual(adapter.session_id, "test-session-1234")
        self.assertEqual(adapter.cwd, self.test_dir)
        self.assertEqual(adapter.model, "claude-opus-4-7")
        self.assertEqual(adapter.permission_mode, "auto")
        self.assertEqual(adapter.source_agent, "agent:claude-code/main@2.1.150")

        # known_tools populated from init list
        self.assertIn("Task", adapter.known_tools)
        self.assertIn("Bash", adapter.known_tools)

        # Assert the synthesized user_prompt was enqueued and posted
        mock_post.assert_called_once()
        posted = mock_post.call_args[1]["json"]
        self.assertEqual(posted["tool_name"], "user_prompt")
        self.assertEqual(posted["source_agent"], "human:claude-code-user")
        self.assertEqual(posted["args"]["prompt"], "[adapter-synthesized: original CLI prompt not captured in stream-json; see v1.2]")

    @patch("requests.post")
    def test_user_prompt_becomes_root(self, mock_post):
        mock_res = MagicMock()
        mock_res.status_code = 200
        mock_res.json.return_value = {"seq_id": 5}
        mock_post.return_value = mock_res

        adapter.last_local_user_or_result_seq = None

        # Parse user prompt
        user_prompt_json = {
            "type": "user",
            "message": {
                "role": "user",
                "content": "Verify all tests compile green"
            }
        }
        adapter.parse_stream_line(json.dumps(user_prompt_json))
        adapter.event_queue.join()

        # Verify it was enqueued and posted as user_prompt with triggered_by=None
        mock_post.assert_called_once()
        posted_data = mock_post.call_args[1]["json"]
        self.assertEqual(posted_data["tool_name"], "user_prompt")
        self.assertEqual(posted_data["source_agent"], "human:claude-code-user")
        self.assertEqual(posted_data["args"]["prompt"], "Verify all tests compile green")
        self.assertIsNone(posted_data["triggered_by"])

        # Verify local sequentials are recorded properly
        self.assertEqual(adapter.local_to_server_seq[adapter.last_local_user_or_result_seq], 5)

    @patch("requests.post")
    def test_tool_use_paired_with_tool_result(self, mock_post):
        mock_res = MagicMock()
        mock_res.status_code = 200
        mock_res.json.return_value = {"seq_id": 10}
        mock_post.return_value = mock_res

        # Set parent causality pointers on local sequence layer
        adapter.last_local_llm_seq = 4
        adapter.local_to_server_seq[4] = 8

        # Buffer tool use
        tool_use = {
            "id": "toolu_1234",
            "name": "Read",
            "input": {"file_path": "src/web.rs"}
        }
        adapter.pending_tool_uses["toolu_1234"] = (tool_use, time.monotonic() - 0.5) # start 500ms ago

        # Parse tool result user event
        user_result_json = {
            "type": "user",
            "message": {
                "role": "user",
                "content": [
                    {
                        "tool_use_id": "toolu_1234",
                        "type": "tool_result",
                        "content": "fn test_tool_call() {}",
                        "is_error": False
                    }
                ]
            }
        }
        adapter.parse_stream_line(json.dumps(user_result_json))
        adapter.event_queue.join()

        # Verify call was paired and emitted
        mock_post.assert_called_once()
        posted_data = mock_post.call_args[1]["json"]
        self.assertEqual(posted_data["tool_name"], "Read")
        self.assertEqual(posted_data["args"]["file_path"], "src/web.rs")
        self.assertEqual(posted_data["result"], "fn test_tool_call() {}") # direct string result
        self.assertEqual(posted_data["triggered_by"], 8)
        self.assertTrue(posted_data["success"])
        self.assertGreaterEqual(posted_data["duration_ms"], 500)

    @patch("requests.post")
    def test_parallel_tools_share_triggered_by(self, mock_post):
        mock_res = MagicMock()
        mock_res.status_code = 200
        mock_res.json.return_value = {"seq_id": 12}
        mock_post.return_value = mock_res

        # Set parent causality pointers on local sequence layer
        adapter.last_local_llm_seq = 5
        adapter.local_to_server_seq[5] = 10

        # Buffer two tool uses from the same assistant run
        adapter.pending_tool_uses["toolu_A"] = ({"id": "toolu_A", "name": "Read", "input": {"file_path": "a.txt"}}, time.monotonic())
        adapter.pending_tool_uses["toolu_B"] = ({"id": "toolu_B", "name": "Write", "input": {"file_path": "b.txt"}}, time.monotonic())

        # Complete tool result A
        adapter.parse_stream_line(json.dumps({
            "type": "user",
            "message": {"role": "user", "content": [{"tool_use_id": "toolu_A", "type": "tool_result", "content": "res A", "is_error": False}]}
        }))
        adapter.event_queue.join()

        # Complete tool result B
        adapter.parse_stream_line(json.dumps({
            "type": "user",
            "message": {"role": "user", "content": [{"tool_use_id": "toolu_B", "type": "tool_result", "content": "res B", "is_error": False}]}
        }))
        adapter.event_queue.join()

        # Assert both share parent triggered_by = 10
        calls = mock_post.call_args_list
        self.assertEqual(len(calls), 2)
        self.assertEqual(calls[0][1]["json"]["triggered_by"], 10)
        self.assertEqual(calls[1][1]["json"]["triggered_by"], 10)

    @patch("requests.post")
    def test_large_result_content_refs(self, mock_post):
        mock_res = MagicMock()
        mock_res.status_code = 200
        mock_res.json.return_value = {"seq_id": 15}
        mock_post.return_value = mock_res

        # Prepare tool use
        adapter.pending_tool_uses["toolu_large"] = ({"id": "toolu_large", "name": "Bash", "input": {"command": "test"}}, time.monotonic())

        # Generate result string larger than 1024 bytes (1.2KB)
        large_content = "A" * 1200

        adapter.parse_stream_line(json.dumps({
            "type": "user",
            "message": {
                "role": "user",
                "content": [
                    {
                        "tool_use_id": "toolu_large",
                        "type": "tool_result",
                        "content": large_content,
                        "is_error": False
                    }
                ]
            }
        }))
        adapter.event_queue.join()

        # Verify content was extracted as content ref directly replacing the string value
        mock_post.assert_called_once()
        posted_data = mock_post.call_args[1]["json"]
        ref_block = posted_data["result"]
        self.assertIn("_ref", ref_block)
        self.assertEqual(ref_block["size_bytes"], 1200)

        # Verify payload_refs is correctly populated with the blob ContentRef struct
        digest = ref_block["_ref"].replace("sha256:", "")
        self.assertEqual(posted_data["payload_refs"], [{
            "sha256": digest,
            "size_bytes": 1200,
            "label": "Bash"
        }])

        # Verify blob file was physically written to workspace blobs directory
        blob_path = os.path.join(self.test_dir, ".korg", "blobs", digest[:2], digest)
        self.assertTrue(os.path.exists(blob_path))
        with open(blob_path, "r") as f:
            self.assertEqual(f.read(), large_content)

    @patch("requests.post")
    def test_bash_result_uses_content_field_not_tool_use_result(self, mock_post):
        # Tests Option 1 (using user.message.content[].content instead of tool_use_result)
        mock_res = MagicMock()
        mock_res.status_code = 200
        mock_res.json.return_value = {"seq_id": 20}
        mock_post.return_value = mock_res

        adapter.pending_tool_uses["toolu_bash"] = ({"id": "toolu_bash", "name": "Bash", "input": {"command": "ls"}}, time.monotonic())

        # Emitted user event with both content (Option 1 source) and tool_use_result (Option 2 source)
        adapter.parse_stream_line(json.dumps({
            "type": "user",
            "message": {
                "role": "user",
                "content": [
                    {
                        "tool_use_id": "toolu_bash",
                        "type": "tool_result",
                        "content": "OPTION_1_EXPECTED_VALUE",
                        "is_error": False
                    }
                ]
            },
            "tool_use_result": {
                "stdout": "OPTION_2_DISCARDED_VALUE",
                "stderr": "",
                "interrupted": False
            }
        }))
        adapter.event_queue.join()

        mock_post.assert_called_once()
        posted_data = mock_post.call_args[1]["json"]
        self.assertEqual(posted_data["result"], "OPTION_1_EXPECTED_VALUE")

    @patch("requests.post")
    def test_unknown_tool_logged_not_emitted(self, mock_post):
        # Buffer tool use of unsupported tool — known_tools is empty (setUp), so CronCreate is unknown
        unknown_assistant_json = {
            "type": "assistant",
            "message": {
                "model": "claude-opus",
                "id": "msg_unsupported",
                "content": [
                    {
                        "type": "tool_use",
                        "id": "toolu_unknown",
                        "name": "CronCreate",  # not in known_tools (empty after setUp)
                        "input": {"cron_expression": "*/5 * * * *"}
                    }
                ]
            }
        }
        adapter.parse_stream_line(json.dumps(unknown_assistant_json))
        adapter.event_queue.join()

        # Verify it was NOT buffered inside pending_tool_uses
        self.assertNotIn("toolu_unknown", adapter.pending_tool_uses)

        # Verify it was written to unknown_events.log
        self.assertTrue(os.path.exists("unknown_events.log"))
        with open("unknown_events.log", "r") as f:
            logged_lines = f.readlines()
            self.assertEqual(len(logged_lines), 1)
            self.assertIn("CronCreate", logged_lines[0])

    @patch("requests.post")
    def test_session_complete_emitted_on_result_event(self, mock_post):
        mock_res = MagicMock()
        mock_res.status_code = 200
        mock_res.json.return_value = {"seq_id": 35}
        mock_post.return_value = mock_res

        # Set local causal pointers
        adapter.last_local_emitted_seq = 6
        adapter.local_to_server_seq[6] = 30

        adapter.cwd = "/workspace"
        adapter.model = "opus"
        adapter.permission_mode = "auto"

        result_json = {
            "type": "result",
            "subtype": "success",
            "is_error": False,
            "duration_ms": 5000,
            "result": "Listed contents cleanly",
            "terminal_reason": "completed",
            "stop_reason": "end_turn",
            "permission_denials": ["denied-bash"],
            "total_cost_usd": 0.05,
            "num_turns": 4
        }

        adapter.parse_stream_line(json.dumps(result_json))
        adapter.event_queue.join()

        # Assert session_complete enqueued and posted with strict allowlist
        mock_post.assert_called_once()
        posted_data = mock_post.call_args[1]["json"]
        self.assertEqual(posted_data["tool_name"], "session_complete")
        self.assertEqual(posted_data["triggered_by"], 30)
        self.assertEqual(posted_data["args"]["cwd"], "/workspace")
        self.assertEqual(posted_data["args"]["model"], "opus")

        res_payload = posted_data["result"]
        self.assertEqual(res_payload["subtype"], "success")
        self.assertEqual(res_payload["duration_ms"], 5000)
        self.assertEqual(res_payload["total_cost_usd"], 0.05)
        self.assertEqual(res_payload["summary"], "Listed contents cleanly")
        self.assertEqual(res_payload["permission_denials"], ["denied-bash"])
        self.assertEqual(res_payload["num_turns"], 4)

        # Assert pruned properties do not exist
        self.assertNotIn("usage", res_payload)
        self.assertNotIn("modelUsage", res_payload)

    @patch("requests.post")
    def test_orphan_event_when_no_init(self, mock_post):
        # We manually reset init_received to False for testing fallback prompt injection
        adapter.init_received = False

        mock_res = MagicMock()
        mock_res.status_code = 200
        mock_res.json.return_value = {"seq_id": 1}
        mock_post.return_value = mock_res

        # Parse user event directly without init
        adapter.parse_stream_line(json.dumps({
            "type": "user",
            "message": {"role": "user", "content": "Initial prompt without system init"}
        }))
        adapter.event_queue.join()

        self.assertTrue(adapter.init_received)
        self.assertIsNotNone(adapter.session_id)
        self.assertEqual(adapter.source_agent, "agent:claude-code/main@unknown")

        # Expect two posts: 1 for synthesized user_prompt init, 1 for explicit prompt
        self.assertEqual(mock_post.call_count, 2)
        calls = mock_post.call_args_list
        self.assertEqual(calls[0][1]["json"]["tool_name"], "user_prompt")
        self.assertEqual(calls[0][1]["json"]["source_agent"], "human:claude-code-user")
        self.assertEqual(calls[0][1]["json"]["args"]["prompt"], "[adapter-synthesized: original CLI prompt not captured in stream-json; see v1.2]")
        self.assertEqual(calls[1][1]["json"]["tool_name"], "user_prompt")
        self.assertEqual(calls[1][1]["json"]["source_agent"], "human:claude-code-user")
        self.assertEqual(calls[1][1]["json"]["args"]["prompt"], "Initial prompt without system init")

    @patch("requests.post")
    def test_user_prompt_emitted_as_root(self, mock_post):
        adapter.init_received = False

        mock_res = MagicMock()
        mock_res.status_code = 200
        mock_res.json.return_value = {"seq_id": 1}
        mock_post.return_value = mock_res

        # Parse system init event, which synthesizes a user_prompt root event
        adapter.parse_stream_line(json.dumps({
            "type": "system",
            "subtype": "init",
            "session_id": "root-session-id"
        }))
        adapter.event_queue.join()

        mock_post.assert_called_once()
        posted_data = mock_post.call_args[1]["json"]
        self.assertEqual(posted_data["tool_name"], "user_prompt")
        self.assertEqual(posted_data["source_agent"], "human:claude-code-user")
        self.assertEqual(posted_data["args"]["prompt"], "[adapter-synthesized: original CLI prompt not captured in stream-json; see v1.2]")
        self.assertIsNone(posted_data["triggered_by"])

    @patch("requests.post")
    def test_korg_unreachable_does_not_block(self, mock_post):
        # Mock requests failure throwing exception
        mock_post.side_effect = Exception("Connection refused")

        # Parse user event
        adapter.parse_stream_line(json.dumps({
            "type": "user",
            "message": {"role": "user", "content": "Fire-and-forget prompt when server is dead"}
        }))

        # Wait for queue to flush
        adapter.event_queue.join()

        # If it returns here without raising, the test succeeded, confirming
        # that unreachable Korg server is non-blocking to the parser/stdin stream.
        self.assertTrue(mock_post.called)

    @patch("requests.post")
    def test_live_fixture_integration_causality(self, mock_post):
        # We will parse the entire tests/fixtures/sample_session.jsonl file
        # and verify the sequence of POSTs, causal links, and payload_refs.

        adapter.init_received = False

        # Mock successful sequential seq_ids from Korg server:
        # 1. user_prompt (synthesized root) -> seq_id=403
        # 2. llm_inference (first assistant turn) -> seq_id=404
        # 3. Bash tool call (first user tool result) -> seq_id=405
        # 4. llm_inference (second assistant turn) -> seq_id=406
        # 5. session_complete (result event) -> seq_id=407
        seq_ids = [403, 404, 405, 406, 407]
        call_count = 0

        def mock_post_handler(url, json=None, headers=None, timeout=5.0):
            nonlocal call_count
            res = MagicMock()
            res.status_code = 200
            res.json.return_value = {"seq_id": seq_ids[call_count]}
            call_count += 1
            return res

        mock_post.side_effect = mock_post_handler

        fixture_path = os.path.join(os.path.dirname(__file__), "fixtures", "sample_session.jsonl")
        with open(fixture_path, "r") as f:
            for line in f:
                adapter.parse_stream_line(line)

        adapter.event_queue.join()

        # Verify 5 events were enqueued and posted
        self.assertEqual(call_count, 5)

        calls = [args[1]["json"] for args in mock_post.call_args_list]

        # 1. user_prompt synthesized on system/init
        self.assertEqual(calls[0]["tool_name"], "user_prompt")
        self.assertEqual(calls[0]["source_agent"], "human:claude-code-user")
        self.assertEqual(calls[0]["args"]["prompt"], "[adapter-synthesized: original CLI prompt not captured in stream-json; see v1.2]")
        self.assertIsNone(calls[0]["triggered_by"])

        # 2. llm_inference from first assistant turn
        self.assertEqual(calls[1]["tool_name"], "llm_inference")
        self.assertEqual(calls[1]["triggered_by"], 403) # Chains back to user_prompt root!

        # 3. Bash tool call from tool result (with large chunked result and structured ContentRef)
        self.assertEqual(calls[2]["tool_name"], "Bash")
        self.assertEqual(calls[2]["triggered_by"], 404) # Chains back to its llm_inference parent!

        ref_block = calls[2]["result"]
        self.assertIn("_ref", ref_block)
        self.assertEqual(ref_block["size_bytes"], 4484)

        # Verify payload_refs contains the compliant ContentRef dictionary structure
        digest = ref_block["_ref"].replace("sha256:", "")
        self.assertEqual(calls[2]["payload_refs"], [{
            "sha256": digest,
            "size_bytes": 4484,
            "label": "Bash"
        }])

        # 4. llm_inference from second assistant turn
        self.assertEqual(calls[3]["tool_name"], "llm_inference")
        self.assertEqual(calls[3]["triggered_by"], 405) # Chains back to Bash tool result!

        # 5. session_complete from result event
        self.assertEqual(calls[4]["tool_name"], "session_complete")
        self.assertEqual(calls[4]["triggered_by"], 406) # Chains back to last llm_inference!

    @patch("requests.post")
    def test_subagent_fixture_causality(self, mock_post):
        # Parse subagent_session.jsonl (15 events) and verify sub-agent causal chain.
        #
        # Expected 8 events (in order):
        #   1. user_prompt  (main, synthesized on system/init)        triggered_by=None
        #   2. llm_inference (main, first assistant turn)             triggered_by=seq(1)
        #   3. user_prompt  (sub, ptuid=toolu_01A...)                 triggered_by=seq(2)  [cross-spine]
        #   4. llm_inference (sub, Explore agent turn)                triggered_by=seq(3)
        #   5. Bash         (sub, tool result)                        triggered_by=seq(4)
        #   6. Agent        (main, Agent tool_result)                 triggered_by=seq(2)  [main last_llm]
        #   7. llm_inference (main, final text turn)                  triggered_by=seq(6)
        #   8. session_complete                                       triggered_by=seq(7)

        adapter.init_received = False

        seq_ids = [100, 101, 102, 103, 104, 105, 106, 107]
        call_count = 0

        def mock_post_handler(url, json=None, headers=None, timeout=5.0):
            nonlocal call_count
            res = MagicMock()
            res.status_code = 200
            res.json.return_value = {"seq_id": seq_ids[call_count]}
            call_count += 1
            return res

        mock_post.side_effect = mock_post_handler

        fixture_path = os.path.join(os.path.dirname(__file__), "fixtures", "subagent_session.jsonl")
        with open(fixture_path, "r") as f:
            for line in f:
                adapter.parse_stream_line(line)

        adapter.event_queue.join()

        self.assertEqual(call_count, 8)

        calls = [args[1]["json"] for args in mock_post.call_args_list]

        # 1. Synthesized main-spine user_prompt
        self.assertEqual(calls[0]["tool_name"], "user_prompt")
        self.assertEqual(calls[0]["source_agent"], "human:claude-code-user")
        self.assertIsNone(calls[0]["triggered_by"])

        # 2. Main-spine llm_inference
        self.assertEqual(calls[1]["tool_name"], "llm_inference")
        self.assertEqual(calls[1]["source_agent"], "agent:claude-code/main@2.1.150")
        self.assertEqual(calls[1]["triggered_by"], 100)

        # 3. Sub-agent user_prompt — cross-spine link to main llm_inference
        self.assertEqual(calls[2]["tool_name"], "user_prompt")
        self.assertEqual(calls[2]["source_agent"], "agent:claude-code/sub-toolu_01@2.1.150")
        self.assertEqual(calls[2]["triggered_by"], 101)  # main last_llm_seq

        # 4. Sub-agent llm_inference
        self.assertEqual(calls[3]["tool_name"], "llm_inference")
        self.assertEqual(calls[3]["source_agent"], "agent:claude-code/sub-toolu_01@2.1.150")
        self.assertEqual(calls[3]["triggered_by"], 102)  # sub last_user_or_result_seq

        # 5. Sub-agent Bash tool result
        self.assertEqual(calls[4]["tool_name"], "Bash")
        self.assertEqual(calls[4]["source_agent"], "agent:claude-code/sub-toolu_01@2.1.150")
        self.assertEqual(calls[4]["triggered_by"], 103)  # sub last_llm_seq

        # 6. Main-spine Agent tool result (completes the Agent/Task tool call)
        self.assertEqual(calls[5]["tool_name"], "Agent")
        self.assertEqual(calls[5]["source_agent"], "agent:claude-code/main@2.1.150")
        self.assertEqual(calls[5]["triggered_by"], 101)  # main last_llm_seq (set at event 2)

        # 7. Main-spine llm_inference (final text turn)
        self.assertEqual(calls[6]["tool_name"], "llm_inference")
        self.assertEqual(calls[6]["source_agent"], "agent:claude-code/main@2.1.150")
        self.assertEqual(calls[6]["triggered_by"], 105)  # main last_user_or_result_seq (updated by Agent result)

        # 8. session_complete
        self.assertEqual(calls[7]["tool_name"], "session_complete")
        self.assertEqual(calls[7]["triggered_by"], 106)  # main last_emitted_seq

if __name__ == "__main__":
    unittest.main()
