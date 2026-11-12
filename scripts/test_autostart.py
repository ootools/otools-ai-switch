import json
import os
import socket
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# A new process models a host restart and never reads the user's plugin data.
HOST_TASK = r"""
import ctypes, json, os, pathlib, socket, sys
root = pathlib.Path(sys.argv[1])
library = pathlib.Path(sys.argv[2])
manifest = json.loads((root / "plugin.json").read_text(encoding="utf-8"))
native = ctypes.CDLL(str(library))
native.otools_plugin_invoke.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
native.otools_plugin_invoke.restype = ctypes.c_void_p
native.otools_plugin_free.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
native.otools_plugin_free.restype = None

def call(method, payload=None):
    encoded = json.dumps({"method": method, "payload": payload or {}}).encode()
    buffer = ctypes.create_string_buffer(encoded)
    length = ctypes.c_size_t()
    ptr = native.otools_plugin_invoke(buffer, len(encoded), ctypes.byref(length))
    if not ptr:
        raise RuntimeError("Null native response")
    try:
        return json.loads(ctypes.string_at(ptr, length.value))
    finally:
        native.otools_plugin_free(ptr, length.value)

results = []
try:
    for task in manifest.get("autostart", {}).get("tasks", []):
        results.append(call(task["command"], task.get("args", {})))
    status = call("get_route_proxy_status")
    if sys.argv[3] == "duplicate":
        for task in manifest.get("autostart", {}).get("tasks", []):
            results.append(call(task["command"], task.get("args", {})))
    connected = False
    with socket.socket() as check:
        connected = check.connect_ex(("127.0.0.1", 29527)) == 0
    # Report only the public service state, never local keys or account details.
    print(json.dumps({"results": [{"ok": value.get("ok"), "error": value.get("error")} for value in results], "running": status.get("data", {}).get("running"), "port": status.get("data", {}).get("port"), "connected": connected}))
finally:
    call("__otools_shutdown__")
"""


class PluginAutostartTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.library = Path(os.environ.get("OTOOLS_AI_SWITCH_TEST_LIBRARY", str(ROOT / "lib/Windows.dll"))).resolve()
        if not cls.library.is_file():
            raise RuntimeError("Build the native library before running autostart tests")
        with socket.socket() as port:
            if port.connect_ex(("127.0.0.1", 29527)) == 0:
                raise RuntimeError("Stop the test AI listener before running autostart tests; port 29527 is in use")

    def run_host(self, data_dir, mode="once"):
        env = {**os.environ, "OTOOLS_AI_SWITCH_DATA_DIR": str(data_dir)}
        result = subprocess.run(
            [sys.executable, "-I", "-c", HOST_TASK, str(ROOT), str(self.library), mode],
            capture_output=True, text=True, env=env, timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout.strip())

    def test_host_starts_ai_service_before_opening_panel_and_after_restart(self):
        with tempfile.TemporaryDirectory(prefix="otools-ai-switch-autostart-") as directory:
            for cycle in range(2):
                state = self.run_host(directory)
                self.assertTrue(state["results"], "No native autostart task is configured")
                self.assertTrue(all(value["ok"] for value in state["results"]), state["results"])
                self.assertTrue(state["running"], f"Host restart {cycle}: listener not started")
                self.assertEqual(state["port"], 29527)
                self.assertTrue(state["connected"])

    def test_repeated_startup_is_idempotent(self):
        with tempfile.TemporaryDirectory(prefix="otools-ai-switch-autostart-") as directory:
            state = self.run_host(directory, "duplicate")
            self.assertEqual(len(state["results"]), 2)
            self.assertTrue(all(value["ok"] for value in state["results"]), state["results"])
            self.assertTrue(state["running"])

    def test_port_conflict_is_reported_by_the_background_task(self):
        with tempfile.TemporaryDirectory(prefix="otools-ai-switch-autostart-") as directory, socket.socket() as port:
            if hasattr(socket, "SO_EXCLUSIVEADDRUSE"):
                port.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
            else:
                # Earlier host processes probe the listener with connect() before
                # shutting down, which leaves the port in TIME_WAIT on POSIX.
                # SO_REUSEADDR lets us bind that state; a live LISTEN socket from
                # a leaked host still blocks the bind, which is what this test needs.
                port.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            for _ in range(50):
                try:
                    port.bind(("127.0.0.1", 29527))
                    break
                except OSError:
                    time.sleep(0.1)
            else:
                self.fail("port 29527 is still held by a previous test host")
            port.listen(1)
            state = self.run_host(directory)
            self.assertTrue(state["results"], "No native autostart task is configured")
            self.assertFalse(state["results"][0]["ok"])
            self.assertTrue(state["results"][0]["error"])
            self.assertFalse(state["running"])


if __name__ == "__main__":
    unittest.main()
