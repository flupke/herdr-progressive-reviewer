"""Exercise reviewer-control with a real, privately attached Herdr server."""

import fcntl
import json
import os
import pathlib
import pty
import select
import signal
import socket
import struct
import subprocess
import sys
import termios
import textwrap
import time


class ProbePane:
    """Paint width-dependent reply rows and expose the dimensions used for them."""

    @staticmethod
    def run(snapshot):
        previous = None
        sys.stdout.write("\033[?1049h\033[?25l")
        sys.stdout.flush()
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            width, height = os.get_terminal_size()
            if (width, height) != previous:
                words = " ".join(f"word{index:03}" for index in range(60))
                lines = ["|" + row.ljust(width - 2) + "|"
                         for row in textwrap.wrap(words, width=width - 2)]
                assert len(lines) < height, (width, height)
                sys.stdout.write("\033[2J" + "".join(
                    f"\033[{index + 1};1H{row}" for index, row in enumerate(lines)))
                sys.stdout.flush()
                temporary = snapshot.with_suffix(".pending")
                temporary.write_text(json.dumps(dict(width=width, height=height, lines=lines)))
                temporary.replace(snapshot)
                previous = (width, height)
            time.sleep(0.02)


class IsolatedHerdr:
    def __init__(self, root):
        self.root = root
        self.server = None
        self.client = None
        self.master = None
        self.log = None
        self.binary = os.environ.get("HERDR_BIN_PATH", "herdr")
        self.env = {key: value for key, value in os.environ.items()
                    if not key.startswith("HERDR_")}
        for name in ("config", "runtime", "state", "plugin", "work", "plugin-state"):
            (root / name).mkdir()
        self.env.update(HERDR_SOCKET_PATH=str(root / "api.sock"),
                        HERDR_CONFIG_PATH=str(root / "config/config.toml"),
                        XDG_CONFIG_HOME=str(root / "config"),
                        XDG_RUNTIME_DIR=str(root / "runtime"),
                        XDG_STATE_HOME=str(root / "state"),
                        SHELL="/bin/sh", TERM="xterm-256color")
        (root / "config/config.toml").write_text(
            'onboarding = false\n[ui]\npane_borders = "always"\n'
            'pane_outer_borders = true\npane_scrollbars = false\npane_gaps = true\n')

    def command(self, arguments, env=None):
        result = subprocess.run(arguments, cwd=self.root / "work", env=env or self.env,
                                capture_output=True, text=True, timeout=8)
        assert result.returncode == 0, (arguments, result.stdout, result.stderr)
        return result.stdout

    def request(self, method, params=None):
        with socket.socket(socket.AF_UNIX) as connection:
            connection.settimeout(2)
            connection.connect(self.env["HERDR_SOCKET_PATH"])
            connection.sendall((json.dumps(dict(id="geometry", method=method,
                                                params=params or {})) + "\n").encode())
            response = b""
            while b"\n" not in response:
                chunk = connection.recv(65536)
                assert chunk, "Herdr closed its response socket"
                response += chunk
            result = json.loads(response.split(b"\n")[0])
            assert "error" not in result, result
            return result["result"]

    def wait_until(self, predicate, description):
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if self.master is not None:
                while select.select([self.master], [], [], 0)[0]:
                    os.read(self.master, 65536)
            if self.server.poll() is not None:
                raise AssertionError((self.root / "server.log").read_text())
            result = predicate()
            if result:
                return result
            time.sleep(0.02)
        raise AssertionError(f"Timed out waiting for {description}")

    def start(self):
        self.command(["git", "init", "--quiet"])
        self.log = (self.root / "server.log").open("wb")
        self.server = subprocess.Popen([self.binary, "server"], cwd=self.root / "work",
                                       env=self.env, stdin=subprocess.DEVNULL,
                                       stdout=self.log, stderr=self.log)
        self.wait_until(lambda: (self.root / "api.sock").exists(), "private server socket")
        workspace = self.request("workspace.create", dict(cwd=str(self.root / "work"),
                                                          label="pane-geometry", focus=True))
        self.workspace = workspace["workspace"]["workspace_id"]
        self.original = workspace["root_pane"]["pane_id"]
        self.client, self.master = pty.fork()
        if self.client == 0:
            fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", 60, 160, 0, 0))
            os.chdir(self.root / "work")
            os.execvpe(self.binary, [self.binary], self.env)
        self.wait_until(lambda: self.layout()["area"]["height"] > 50,
                        "attached client's terminal geometry")

    def layout(self):
        return self.request("pane.layout", dict(pane_id=self.original))["layout"]

    def check_open(self, control):
        plugin = self.root / "plugin"
        snapshot = plugin / "paint.json"
        command = [sys.executable, str(pathlib.Path(__file__).resolve()), "--pane", str(snapshot)]
        (plugin / "herdr-plugin.toml").write_text(
            'id = "herdr.progressive-reviewer"\nname = "Geometry probe"\n'
            'version = "0.1.0"\nmin_herdr_version = "0.7.5"\n'
            '[[panes]]\nid = "review"\ntitle = "Geometry probe"\n'
            f'command = {json.dumps(command)}\nplacement = "split"\n')
        self.request("plugin.link", dict(path=str(plugin), enabled=True))
        context = dict(workspace_id=self.workspace, focused_pane_id=self.original,
                       focused_pane_cwd=str(self.root / "work"))
        environment = dict(self.env, HERDR_PLUGIN_ID="herdr.progressive-reviewer",
                           HERDR_PLUGIN_STATE_DIR=str(self.root / "plugin-state"),
                           HERDR_PLUGIN_CONTEXT_JSON=json.dumps(context))
        self.command([control, "open"], environment)
        layout = self.layout()
        assert len(layout["panes"]) == 2 and not layout["zoomed"], layout
        assert len(layout["splits"]) == 1 and layout["splits"][0]["ratio"] == 0.5, layout
        pane = next(pane for pane in layout["panes"] if pane["pane_id"] != self.original)
        assert layout["focused_pane_id"] == pane["pane_id"], layout
        expected = (pane["rect"]["width"] - 2, pane["rect"]["height"] - 2)

        def settled_paint():
            if not snapshot.exists():
                return None
            paint = json.loads(snapshot.read_text())
            return paint if (paint["width"], paint["height"]) == expected else None

        paint = self.wait_until(settled_paint, f"initial reply layout at {expected}")
        assert all(len(line) == expected[0] and line.endswith("|") for line in paint["lines"])
        expected_text = "\n".join(paint["lines"])
        self.wait_until(lambda: self.request("pane.read", dict(pane_id=pane["pane_id"],
                                                               source="visible"))["read"]["text"]
                        .startswith(expected_text), "unclipped reply rows in Herdr")
        assert self.layout() == layout, "Startup synchronization changed layout or focus"
        print(f"Review split paints all 60 words at {expected}, without further input or resize")

    def close(self):
        if self.client:
            try:
                os.kill(self.client, signal.SIGKILL)
            except ProcessLookupError:
                pass
            os.waitpid(self.client, 0)
        if self.master is not None:
            os.close(self.master)
        if self.server is not None:
            self.server.terminate()
            try:
                self.server.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.server.kill()
                self.server.wait()
        if self.log is not None:
            self.log.close()


if sys.argv[1] == "--pane":
    ProbePane.run(pathlib.Path(sys.argv[2]))
else:
    fixture = IsolatedHerdr(pathlib.Path(sys.argv[1]))
    try:
        fixture.start()
        fixture.check_open(sys.argv[2])
    finally:
        fixture.close()
