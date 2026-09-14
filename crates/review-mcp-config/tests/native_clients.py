"""Exercise registered project configuration without contacting a model provider."""

import http.server
import json
import os
import pathlib
import shutil
import subprocess
import sys
import threading

from native_session import Session

root = pathlib.Path(sys.argv[1])
workspace = root / "workspace"
for name in ("workspace", "codex", "claude", "home", "config", "state", "cache"):
    (root / name).mkdir(exist_ok=True)


class Provider(http.server.BaseHTTPRequestHandler):
    calls = 0
    tool_called = False

    def log_message(self, *_args):
        pass

    def do_POST(self):
        self.rfile.read(int(self.headers["Content-Length"]))
        use_tool = not type(self).tool_called
        type(self).calls += 1
        if use_tool:
            type(self).tool_called = True
            item = {"type": "custom_tool_call", "id": "fc_probe", "call_id": "call_probe", "name": "exec",
                    "namespace": "functions", "input": 'const tool = ALL_TOOLS.find(t => t.name.endsWith("herdr_reviewer__list_threads")); if (tool) text(await tools[tool.name]({review:"native-probe"})); else text("reviewer-unavailable");'}
        else:
            item = {"id": "message-probe", "type": "message", "role": "assistant", "phase": "final_answer",
                    "content": [{"type": "output_text", "text": "Private probe complete.", "annotations": []}]}
        response = {"id": "response-probe", "object": "response", "status": "completed", "output": [item],
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


provider = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Provider)
threading.Thread(target=provider.serve_forever, daemon=True).start()
env = {key: os.environ[key] for key in ("PATH", "LANG", "LD_LIBRARY_PATH") if key in os.environ}
env.update(HOME=str(root / "home"), CODEX_HOME=str(root / "codex"), CLAUDE_CONFIG_DIR=str(root / "claude"),
           XDG_CONFIG_HOME=str(root / "config"), XDG_STATE_HOME=str(root / "state"), XDG_CACHE_HOME=str(root / "cache"),
           DISABLE_AUTOUPDATER="1", DISABLE_NONESSENTIAL_TRAFFIC="1")
(root / "codex" / "config.toml").write_text(f'''
model = "gpt-5.6-terra"
model_provider = "fixture"
approval_policy = "never"
sandbox_mode = "read-only"
[model_providers.fixture]
name = "Private test provider"
base_url = "http://127.0.0.1:{provider.server_port}/v1"
wire_api = "responses"
supports_websockets = false
request_max_retries = 0
stream_max_retries = 0
[projects.{json.dumps(str(workspace))}]
trust_level = "trusted"
''')


def run(arguments):
    result = subprocess.run(arguments, env=env, cwd=workspace, capture_output=True, text=True, timeout=25)
    if result.returncode:
        raise RuntimeError(f"{arguments[0]} failed:\n{result.stdout}\n{result.stderr}")
    return result.stdout + result.stderr


session = None
try:
    codex = shutil.which("codex")
    native = list(pathlib.Path(codex).resolve().parent.parent.glob("node_modules/@openai/codex-linux-*/vendor/*/bin/codex"))
    if len(native) == 1:
        codex = str(native[0])
    configuration_path = workspace / ".codex" / "config.toml"
    configuration = configuration_path.read_text() + '\nstartup_timeout_sec = 2\ntool_timeout_sec = 5\ntools.list_threads.approval_mode = "approve"\n'
    configuration_path.unlink()
    with (root / "codex-stderr.txt").open("w") as stderr:
        session = Session([codex], env, workspace, stderr)
        # Simulate first-time registration after this native conversation already exists.
        configuration_path.write_text(configuration)
        session.request("config/mcpServer/reload", None)
        for phase in ("closed", "available", "closed-again", "reopened"):
            print(phase, flush=True)
            assert sys.stdin.readline().strip() == "continue"
            available = phase in ("available", "reopened")
            Provider.calls = 0
            Provider.tool_called = False
            items = session.turn()
            assert Provider.calls, items
            calls = [item for item in items if item.get("type") == "mcpToolCall"]
            assert len(calls) == 1, items
            assert calls[0]["server"] == "herdr_reviewer", calls
            result = calls[0].get("result")
            assert result is not None, calls
            assert calls[0]["status"] == ("completed" if available else "failed"), calls
            if not available:
                assert "reviewer is closed or unavailable" in json.dumps(result), calls
            if phase == "closed":
                output = run(["claude", "mcp", "get", "herdr_reviewer"])
                assert "Pending approval" in output, output
                (root / "claude" / "settings.json").write_text(json.dumps({"enabledMcpjsonServers": ["herdr_reviewer"]}))
            output = run(["claude", "mcp", "get", "herdr_reviewer"])
            assert "Connected" in output, output
finally:
    if session is not None:
        session.close()
    provider.shutdown()
