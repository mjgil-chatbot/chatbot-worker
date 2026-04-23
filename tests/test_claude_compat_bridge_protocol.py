import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
from concurrent.futures import ThreadPoolExecutor
from types import SimpleNamespace
import unittest

SCRIPT = Path(__file__).resolve().parent.parent / "scripts/claude-compat-chatbot-cli-adapter.py"
spec = importlib.util.spec_from_file_location("claude_compat_protocol_under_test", SCRIPT)
bridge = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = bridge
spec.loader.exec_module(bridge)


@contextlib.contextmanager
def process_with_buffered_output(payloads, exited):
    reader, writer = os.pipe()
    stdout = os.fdopen(reader, "r", encoding="utf-8")
    data = "".join(json.dumps(payload) + "\n" for payload in payloads).encode()
    if data:
        os.write(writer, data)
    if exited:
        os.close(writer)
        writer = None
    process = SimpleNamespace(
        stdin=io.StringIO(), stdout=stdout, stderr=io.StringIO(),
        poll=lambda: 0 if exited else None,
    )
    def close_writer():
        nonlocal writer
        if writer is not None:
            os.close(writer)
            writer = None
    try:
        yield process, close_writer
    finally:
        close_writer()
        stdout.close()
        process.stdin.close()
        process.stderr.close()


class BufferedReplyTests(unittest.TestCase):
    def test_accepts_reply_already_buffered_when_provider_exits(self):
        reply = {"id": "end-session", "result": {"ended": True}}
        request = {"id": "end-session", "method": "session/end"}
        with process_with_buffered_output([reply], exited=True) as (process, _):
            with contextlib.redirect_stdout(io.StringIO()):
                actual = bridge.send_acp_request(process, request, {})
            self.assertEqual(actual, reply)
            self.assertEqual(json.loads(process.stdin.getvalue()), request)

    def test_reads_prefetched_reply_without_waiting_for_another_pipe_write(self):
        notification = {"method": "text_delta", "params": {"text": "hello"}}
        reply = {"id": "prompt", "result": {"accepted": True}}
        state = {}
        captured = io.StringIO()
        with process_with_buffered_output([notification, reply], exited=False) as (process, close):
            def read_reply():
                with contextlib.redirect_stdout(captured):
                    return bridge.send_acp_request(process, {"id": "prompt"}, state)
            with ThreadPoolExecutor(max_workers=1) as pool:
                future = pool.submit(read_reply)
                try:
                    actual = future.result(timeout=1)
                finally:
                    close()
            self.assertEqual(actual, reply)
        self.assertEqual(state["final_text"], "hello")
        self.assertEqual([json.loads(line) for line in captured.getvalue().splitlines()],
                         [{"type": "text_delta", "text": "hello"}])

    def test_eof_without_reply_remains_a_protocol_error(self):
        with process_with_buffered_output([], exited=True) as (process, _):
            with contextlib.redirect_stdout(io.StringIO()):
                actual = bridge.send_acp_request(process, {"id": "missing"}, {})
            self.assertIn("closed stdout", actual["error"]["message"])
            self.assertIn("process exited with code 0", actual["error"]["message"])


if __name__ == "__main__":
    unittest.main()
