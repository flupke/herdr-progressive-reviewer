"""A local Responses provider for a Codex agent: it calls no model.

It prints its port, then answers each request until its standard input closes:

- a prompt that says "call the reviewer" has the agent call the reviewer's list_threads tool;
- an Explore prompt, which names its turn's index, `<directory>/index.md`, beside the round's
  data and the turn's (`round.md`, `turn.md`, as `turn.rs` reads them too), whose access value
  is on a line "Explore review access: <access>", is printed on standard output with that data,
  as one JSON line {"access", "prompt"}; the turn then waits for
  the test's reply, a JSON line {"access", "tool", "arguments"} on standard input, and calls
  the reviewer's tool with those arguments and the access value as "review";
- every other turn, and every turn once its tool answered, ends with the message
  "Answered: <prompt>"; once a tool answered, what it answered is printed on standard output
  first, as one JSON line {"prompt", "output"}.
"""

import http.server
import json
import os
import sys
import threading

replies = {}
replied = threading.Condition()
printing = threading.Lock()


def last_prompt(items):
    """The last prompt of `items`, and whether a tool answered since."""
    for index in range(len(items) - 1, -1, -1):
        item = items[index]
        if item.get("type") == "message" and item.get("role") == "user":
            for part in item.get("content", []):
                if part.get("type") == "input_text" and not part["text"].startswith("<"):
                    answered = any(later.get("type", "").endswith("_output") for later in items[index:])
                    return part["text"], answered
    return "", False


def with_turn(prompt):
    """`prompt` followed by the round's data and the turn's, when it names a turn's index."""
    for word in prompt.split():
        if word.endswith("/index.md"):
            directory = word.removesuffix("/index.md")
            try:
                data = [open(os.path.join(directory, name)).read() for name in ("round.md", "turn.md")]
            except OSError:
                return prompt
            return prompt + "\n" + "\n".join(data)
    return prompt


def access_of(prompt):
    # Codex asks the model for a title of a session's first prompt, which it quotes.
    if not prompt.startswith("Explore round "):
        return None
    for line in prompt.splitlines():
        if line.startswith("Explore review access: "):
            return line.removeprefix("Explore review access: ").strip()
    return None


def call(tool, arguments, call_id):
    """A call of the reviewer's tool `tool` with `arguments`, through Codex's exec tool."""
    script = (f'const tool = ALL_TOOLS.find(t => t.name.endsWith("herdr_reviewer__{tool}")); '
              f'text(tool ? JSON.stringify(await tools[tool.name]({json.dumps(arguments)})) '
              ': "reviewer-unavailable");')
    return {"type": "custom_tool_call", "id": f"fc_{call_id}", "call_id": f"call_{call_id}",
            "name": "exec", "namespace": "functions", "input": script}


def explore_reply(prompt, access):
    """The test's reply to the Explore prompt `prompt`, once it gave it."""
    with printing:
        print(json.dumps({"access": access, "prompt": prompt}), flush=True)
    with replied:
        replied.wait_for(lambda: access in replies)
        return replies[access]


class Provider(http.server.BaseHTTPRequestHandler):
    calls = 0
    counting = threading.Lock()

    def log_message(self, *_args):
        pass

    def do_POST(self):
        items = json.loads(self.rfile.read(int(self.headers["Content-Length"]))).get("input", [])
        with type(self).counting:
            type(self).calls += 1
            call_id = type(self).calls
        prompt, answered = last_prompt(items)
        access = None if answered else access_of(with_turn(prompt))
        if answered:
            outputs = [item.get("output") for item in items if item.get("type", "").endswith("_output")]
            output = outputs[-1]
            if isinstance(output, list):
                output = "\n".join(part.get("text", "") for part in output)
            with printing:
                print(json.dumps({"prompt": prompt, "output": output}), flush=True)
        if access:
            reply = explore_reply(with_turn(prompt), access)
            item = call(reply["tool"], {**reply["arguments"], "review": access}, call_id)
        elif "call the reviewer" in prompt and not answered:
            item = call("list_threads", {"review": "codex-daemon-probe"}, call_id)
        else:
            item = {"id": f"message_{call_id}", "type": "message", "role": "assistant", "phase": "final_answer",
                    "content": [{"type": "output_text", "text": f"Answered: {prompt}", "annotations": []}]}
        response = {"id": f"response_{call_id}", "object": "response", "status": "completed", "output": [item],
                    "usage": {"input_tokens": 10, "input_tokens_details": {"cached_tokens": 4},
                              "output_tokens": 10, "total_tokens": 20}}
        events = [{"type": "response.created", "response": {**response, "status": "in_progress", "output": []}},
                  {"type": "response.output_item.done", "output_index": 0, "item": item},
                  {"type": "response.completed", "response": response}]
        try:
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.end_headers()
            for event in events:
                self.wfile.write(("data: " + json.dumps(event) + "\n\n").encode())
            self.wfile.flush()
        except OSError:
            # Codex gave up on the request, as when a turn is interrupted.
            pass


def serve():
    provider = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider)
    provider.daemon_threads = True
    with printing:
        print(provider.server_port, flush=True)
    provider.serve_forever()


threading.Thread(target=serve, daemon=True).start()
# The test holds the other end: once it lets go, even by dying, the provider stops.
for line in sys.stdin:
    reply = json.loads(line)
    with replied:
        replies[reply["access"]] = reply
        replied.notify_all()
os._exit(0)
