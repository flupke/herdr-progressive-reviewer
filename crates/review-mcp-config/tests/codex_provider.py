"""A local Responses provider for a Codex agent: it calls no model.

It prints its port, then answers each request until its standard input closes: a prompt that
says "call the reviewer" has the agent call the reviewer's list_threads tool first; every other
turn ends with a message.
"""

import http.server
import json
import os
import sys
import threading


def last_prompt(items):
    for item in reversed(items):
        if item.get("type") == "message" and item.get("role") == "user":
            for part in item.get("content", []):
                if part.get("type") == "input_text" and not part["text"].startswith("<"):
                    return part["text"]
    return ""


class Provider(http.server.BaseHTTPRequestHandler):
    calls = 0

    def log_message(self, *_args):
        pass

    def do_POST(self):
        items = json.loads(self.rfile.read(int(self.headers["Content-Length"]))).get("input", [])
        type(self).calls += 1
        call = type(self).calls
        prompt = last_prompt(items)
        answered = bool(items) and items[-1].get("type", "").endswith("_output")
        if "call the reviewer" in prompt and not answered:
            item = {"type": "custom_tool_call", "id": f"fc_{call}", "call_id": f"call_{call}",
                    "name": "exec", "namespace": "functions",
                    "input": 'const tool = ALL_TOOLS.find(t => t.name.endsWith("herdr_reviewer__list_threads")); '
                             'text(tool ? await tools[tool.name]({review: "codex-daemon-probe"}) : "reviewer-unavailable");'}
        else:
            item = {"id": f"message_{call}", "type": "message", "role": "assistant", "phase": "final_answer",
                    "content": [{"type": "output_text", "text": f"Answered: {prompt}", "annotations": []}]}
        response = {"id": f"response_{call}", "object": "response", "status": "completed", "output": [item],
                    "usage": {"input_tokens": 10, "output_tokens": 10, "total_tokens": 20}}
        events = [{"type": "response.created", "response": {**response, "status": "in_progress", "output": []}},
                  {"type": "response.output_item.done", "output_index": 0, "item": item},
                  {"type": "response.completed", "response": response}]
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        for event in events:
            self.wfile.write(("data: " + json.dumps(event) + "\n\n").encode())
        self.wfile.flush()


def serve():
    provider = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider)
    print(provider.server_port, flush=True)
    provider.serve_forever()


threading.Thread(target=serve, daemon=True).start()
# The test holds the other end: once it lets go, even by dying, the provider stops.
sys.stdin.read()
os._exit(0)
