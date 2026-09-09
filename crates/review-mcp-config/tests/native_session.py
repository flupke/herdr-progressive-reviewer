"""A private, persistent native Codex app-server session for MCP lifecycle tests."""

import json
import selectors
import subprocess
import time


class Session:
    def __init__(self, arguments, env, workspace, stderr):
        self.process = subprocess.Popen(arguments + ["app-server"], env=env, cwd=workspace,
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr,
                                        bufsize=0)
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.process.stdout, selectors.EVENT_READ)
        self.buffer = b""
        self.next_id = 0
        try:
            self.request("initialize", {"clientInfo": {"name": "private-mcp-test", "version": "1"},
                                        "capabilities": {"experimentalApi": True}})
            self.send({"method": "initialized"})
            self.thread_id = self.request("thread/start", {"cwd": str(workspace)})["thread"]["id"]
        except BaseException:
            self.close()
            raise

    def send(self, message):
        self.process.stdin.write((json.dumps(message) + "\n").encode())
        self.process.stdin.flush()

    def receive(self, deadline):
        while b"\n" not in self.buffer:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not self.selector.select(remaining):
                raise TimeoutError("Private Codex did not respond")
            chunk = self.process.stdout.read(65536)
            if not chunk:
                raise RuntimeError("Private Codex exited")
            self.buffer += chunk
        line, self.buffer = self.buffer.split(b"\n", 1)
        return json.loads(line)

    def request(self, method, params):
        self.next_id += 1
        self.send({"id": self.next_id, "method": method, "params": params})
        deadline = time.monotonic() + 25
        while True:
            message = self.receive(deadline)
            if message.get("id") == self.next_id:
                assert "error" not in message, message
                return message["result"]

    def turn(self):
        self.request("turn/start", {"threadId": self.thread_id,
                                   "input": [{"type": "text", "text": "Private MCP lifecycle probe"}]})
        completed = []
        deadline = time.monotonic() + 25
        while True:
            message = self.receive(deadline)
            if message.get("method") == "item/completed":
                completed.append(message["params"]["item"])
            if message.get("method") == "turn/completed":
                assert message["params"]["turn"]["status"] == "completed", message
                return completed

    def close(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
        self.selector.close()
