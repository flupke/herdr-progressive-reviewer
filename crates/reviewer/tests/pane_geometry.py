"""Exercise reviewer-control with a real, privately attached Herdr server.

Every wait waits on an event: a line Herdr prints, an event Herdr sends, a paint the probe
pane reports on a socket. Its only clock is a guard that ends a wait that failed.
"""

import fcntl
import json
import os
import pathlib
import pty
import queue
import select
import signal
import socket
import struct
import subprocess
import sys
import termios
import textwrap
import threading

# How long a wait lasts before it fails: a guard that never delays a check that passes.
GUARD = 30


class ProbePane:
    """Paint width-dependent reply rows on start and on each resize, and report the
    dimensions used for each paint to the test's socket."""

    @staticmethod
    def run(address):
        report = socket.socket(socket.AF_UNIX)
        report.connect(str(address))
        signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGWINCH})
        previous = None
        sys.stdout.write("\033[?1049h\033[?25l")
        sys.stdout.flush()
        while True:
            width, height = os.get_terminal_size()
            if (width, height) != previous:
                words = " ".join(f"word{index:03}" for index in range(60))
                lines = ["|" + row.ljust(width - 2) + "|"
                         for row in textwrap.wrap(words, width=width - 2)]
                assert len(lines) < height, (width, height)
                sys.stdout.write("\033[2J" + "".join(
                    f"\033[{index + 1};1H{row}" for index, row in enumerate(lines)))
                sys.stdout.flush()
                report.sendall((json.dumps(dict(width=width, height=height,
                                                lines=lines)) + "\n").encode())
                previous = (width, height)
            # The pane runs until Herdr closes it.
            signal.sigwait({signal.SIGWINCH})


class Lines:
    """The lines a socket or a pipe delivers, read within the guard."""

    def __init__(self, source):
        self.source = source
        self.buffer = b""

    def next(self, description):
        while b"\n" not in self.buffer:
            if not select.select([self.source], [], [], GUARD)[0]:
                raise AssertionError(f"Timed out waiting for {description}")
            chunk = (self.source.recv(65536) if isinstance(self.source, socket.socket)
                     else os.read(self.source.fileno(), 65536))
            if not chunk:
                raise AssertionError(f"Closed while waiting for {description}")
            self.buffer += chunk
        line, self.buffer = self.buffer.split(b"\n", 1)
        return line.decode()


class IsolatedHerdr:
    def __init__(self, root):
        self.root = root
        self.server = None
        self.client = None
        self.master = None
        self.log = None
        # The release the dev shell pins, as in review-test-support.
        self.binary = os.environ.get("TEST_HERDR_BIN_PATH", "herdr")
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

    def request(self, method, params=None, timeout=2):
        with socket.socket(socket.AF_UNIX) as connection:
            connection.settimeout(timeout)
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

    def start(self):
        self.command(["git", "init", "--quiet"])
        self.log = (self.root / "server.log").open("wb")
        self.server = subprocess.Popen([self.binary, "server"], cwd=self.root / "work",
                                       env=self.env, stdin=subprocess.DEVNULL,
                                       stdout=self.log, stderr=subprocess.PIPE)
        output = Lines(self.server.stderr)
        while "herdr server running" not in (line := output.next("the private server")):
            self.log.write((line + "\n").encode())
        self.log.write(output.buffer)
        self.log.flush()
        threading.Thread(target=self.copy_to_log, args=(self.server.stderr,),
                         daemon=True).start()
        workspace = self.request("workspace.create", dict(cwd=str(self.root / "work"),
                                                          label="pane-geometry", focus=True))
        self.workspace = workspace["workspace"]["workspace_id"]
        self.original = workspace["root_pane"]["pane_id"]
        self.client, self.master = pty.fork()
        if self.client == 0:
            fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", 60, 220, 0, 0))
            os.chdir(self.root / "work")
            os.execvpe(self.binary, [self.binary], self.env)
        # The attached client draws on its terminal, which must be read for it to go on. Herdr
        # sends no event when a client's terminal sizes the layout, but draws a frame after.
        frames = queue.Queue()
        threading.Thread(target=self.drain, args=(frames,), daemon=True).start()
        while self.layout()["area"]["height"] <= 50:
            try:
                frames.get(timeout=GUARD)
            except queue.Empty:
                raise AssertionError("Timed out waiting for attached client's terminal geometry")

    def copy_to_log(self, stream):
        for line in stream:
            self.log.write(line)
            self.log.flush()

    def drain(self, frames):
        try:
            while os.read(self.master, 65536):
                frames.put(None)
        except OSError:
            pass

    def layout(self):
        return self.request("pane.layout", dict(pane_id=self.original))["layout"]

    def check_open(self, control, scenario):
        # A lone 220x60 pane looks wide; one beside a neighbour looks tall.
        expected_direction = dict(wide="right", beside_a_neighbour="down")[scenario]
        if scenario == "beside_a_neighbour":
            self.request("pane.split", dict(target_pane_id=self.original, direction="right"))
        before = self.layout()
        known = {pane["pane_id"] for pane in before["panes"]}
        plugin = self.root / "plugin"
        paints = socket.socket(socket.AF_UNIX)
        paints.bind(str(self.root / "paints.sock"))
        paints.listen(1)
        command = [sys.executable, str(pathlib.Path(__file__).resolve()), "--pane",
                   str(self.root / "paints.sock")]
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
        assert len(layout["panes"]) == len(known) + 1 and not layout["zoomed"], layout
        assert len(layout["splits"]) == len(before["splits"]) + 1, layout
        assert any(split["direction"] == expected_direction and split["ratio"] == 0.5
                   for split in layout["splits"]), layout
        pane = next(pane for pane in layout["panes"] if pane["pane_id"] not in known)
        target = next(pane for pane in layout["panes"] if pane["pane_id"] == self.original)
        beside = (pane["rect"]["x"] > target["rect"]["x"]
                  and pane["rect"]["y"] == target["rect"]["y"])
        below = (pane["rect"]["y"] > target["rect"]["y"]
                 and pane["rect"]["x"] == target["rect"]["x"])
        assert beside if expected_direction == "right" else below, layout
        assert layout["focused_pane_id"] == pane["pane_id"], layout
        expected = (pane["rect"]["width"] - 2, pane["rect"]["height"] - 2)
        if not select.select([paints], [], [], GUARD)[0]:
            raise AssertionError("Timed out waiting for the probe pane")
        probe, _ = paints.accept()
        painted = Lines(probe)
        description = f"initial reply layout at {expected}"
        while (paint := json.loads(painted.next(description))) and \
                (paint["width"], paint["height"]) != expected:
            pass
        assert all(len(line) == expected[0] and line.endswith("|") for line in paint["lines"])
        expected_text = "\n".join(paint["lines"])
        # Herdr matches one line at a time: wait for the last row, painted with the others.
        self.request("pane.wait_for_output", dict(
            pane_id=pane["pane_id"], source="visible", timeout_ms=GUARD * 1000,
            match=dict(type="substring", value=paint["lines"][-1])), timeout=GUARD + 5)
        visible = self.request("pane.read", dict(pane_id=pane["pane_id"],
                                                 source="visible"))["read"]["text"]
        assert visible.startswith(expected_text), ("unclipped reply rows in Herdr", visible)
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
        fixture.check_open(sys.argv[2], sys.argv[3])
    finally:
        fixture.close()
