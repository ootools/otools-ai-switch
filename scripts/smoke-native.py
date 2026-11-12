"""Load the real plugin library and exercise it with an isolated temporary database."""

import argparse
import ctypes
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import socket
import tempfile
import threading
import time
import urllib.error
import urllib.request


def check_proxy_forwarding(call, opener, key):
    received = []
    held_request = threading.Event()
    release_request = threading.Event()

    class MockUpstream(BaseHTTPRequestHandler):
        def log_message(self, *unused):
            pass

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            received.append((self.path, self.headers.get("Authorization"), body))
            if body.get("messages", [{}])[0].get("content") == "hold until shutdown":
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.end_headers()
                self.wfile.write(b': waiting\n\n')
                self.wfile.flush()
                held_request.set()
                release_request.wait(timeout=15)
                return
            if body.get("stream"):
                chunks = [
                    {"id": "chatcmpl-smoke-stream", "object": "chat.completion.chunk", "model": "upstream-chat", "choices": [{"index": 0, "delta": {"role": "assistant", "content": "smoke-sse"}, "finish_reason": None}]},
                    {"id": "chatcmpl-smoke-stream", "object": "chat.completion.chunk", "model": "upstream-chat", "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}], "usage": {"prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5}},
                ]
                data = ("".join("data: " + json.dumps(chunk) + "\n\n" for chunk in chunks) + "data: [DONE]\n\n").encode()
                mime = "text/event-stream"
            else:
                data = json.dumps({"id": "chatcmpl-smoke-json", "object": "chat.completion", "model": "upstream-chat", "choices": [{"index": 0, "message": {"role": "assistant", "content": "smoke-json"}, "finish_reason": "stop"}], "usage": {"prompt_tokens": 3, "completion_tokens": 2, "total_tokens": 5}}).encode()
                mime = "application/json"
            self.send_response(200)
            self.send_header("Content-Type", mime)
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

    upstream = ThreadingHTTPServer(("127.0.0.1", 0), MockUpstream)
    worker = threading.Thread(target=upstream.serve_forever, daemon=True)
    worker.start()
    try:
        account = call("create_api_route_credential", {"input": {
            "platform": "codex", "display_name": "Isolated smoke upstream",
            "api_key": "sk-smoke-upstream", "base_url": f"http://127.0.0.1:{upstream.server_port}/v1",
            "interface_format": "openai", "model_mappings_json": '[{"from":"demo-chat","to":"upstream-chat"}]',
            "preview_json": None, "batch_id": None,
        }})
        call("set_route_pool_members", {"input": {"platform": "codex", "account_ids": [account["id"]]}})
        models = call("list_route_proxy_models", {"platform": "codex"})
        assert any(model["id"] == "demo-chat" for model in models), "Native model listing lost the pool model"
        for stream in [False, True]:
            body = {"model": "demo-chat", "messages": [{"role": "user", "content": "local test only"}], "stream": stream}
            request = urllib.request.Request(
                "http://127.0.0.1:29527/v1/chat/completions", data=json.dumps(body).encode(),
                headers={"Content-Type": "application/json", "Authorization": f"Bearer {key}"},
            )
            with opener.open(request, timeout=30) as response:
                data = response.read()
                assert response.status == 200
                if stream:
                    assert "text/event-stream" in response.headers.get("Content-Type", "")
                    assert b"smoke-sse" in data and b"[DONE]" in data
                else:
                    assert json.loads(data)["choices"][0]["message"]["content"] == "smoke-json"
        assert len(received) == 2, f"Unexpected upstream request count: {len(received)}"
        for path, authorization, body in received:
            assert path == "/v1/chat/completions"
            assert authorization == "Bearer sk-smoke-upstream"
            assert body["model"] == "upstream-chat", "Pool model was not mapped to its upstream model"
        pool = call("get_route_pool", {"platform": "codex"})
        assert pool["stats"]["request_count"] >= 2, "Forwarded requests were not recorded in usage history"
        print("PASS: local mock upstream, model mapping, JSON/SSE forwarding and usage recording")
        errors = []

        def held_client():
            request = urllib.request.Request(
                "http://127.0.0.1:29527/v1/chat/completions",
                data=json.dumps({"model": "demo-chat", "messages": [{"role": "user", "content": "hold until shutdown"}], "stream": True}).encode(),
                headers={"Content-Type": "application/json", "Authorization": f"Bearer {key}"},
            )
            try:
                with opener.open(request, timeout=15) as response:
                    response.read()
            except Exception as error:
                errors.append(type(error).__name__)

        client = threading.Thread(target=held_client, daemon=True)
        client.start()
        assert held_request.wait(timeout=10), "Streaming request never reached mock upstream"
        shutdown_started = time.monotonic()
        call("__otools_shutdown__")
        elapsed = time.monotonic() - shutdown_started
        assert elapsed < 3, f"Active-stream shutdown took {elapsed:.2f}s, exceeding host budget"
        release_request.set()
        client.join(timeout=3)
        assert not client.is_alive(), "The model client remained connected after shutdown"
        call("__otools_initialize__")
        print("PASS: active SSE connection cancelled and plugin stopped within the host budget")
    finally:
        release_request.set()
        upstream.shutdown()
        upstream.server_close()
        worker.join(timeout=5)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--library", type=Path, default=Path(__file__).resolve().parents[1] / "lib" / "Windows.dll")
    args = parser.parse_args()
    library = args.library.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="otools-ai-switch-smoke-") as directory:
        os.environ["OTOOLS_AI_SWITCH_DATA_DIR"] = directory
        native = ctypes.CDLL(str(library))
        native.otools_plugin_invoke.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
        native.otools_plugin_invoke.restype = ctypes.c_void_p
        native.otools_plugin_free.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
        native.otools_plugin_free.restype = None

        def raw_call(method, payload=None):
            encoded = json.dumps({"method": method, "payload": payload or {}}).encode()
            buffer = ctypes.create_string_buffer(encoded)
            size = ctypes.c_size_t()
            ptr = native.otools_plugin_invoke(buffer, len(encoded), ctypes.byref(size))
            assert ptr, f"{method}: null FFI response"
            try:
                return json.loads(ctypes.string_at(ptr, size.value))
            finally:
                native.otools_plugin_free(ptr, size.value)

        def call(method, payload=None):
            result = raw_call(method, payload)
            assert result.get("ok") is True, f"{method}: {result.get('error')}"
            return result.get("data")

        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

        def http(path, headers=None):
            request = urllib.request.Request(f"http://127.0.0.1:29527{path}", headers=headers or {})
            try:
                with opener.open(request, timeout=10) as response:
                    return response.status, response.read()
            except urllib.error.HTTPError as error:
                return error.code, error.read()

        def assert_port_released():
            with socket.socket() as check:
                assert check.connect_ex(("127.0.0.1", 29527)) != 0, "AI port remains open after stopping"

        try:
            settings = call("get_settings")
            assert isinstance(settings, dict)
            assert call("list_route_credentials", {"platform": "codex"}) == []
            assert call("get_route_proxy_status")["running"] is False
            assert isinstance(call("poll_events")["events"], list)
            assert raw_call("nonexistent_method")["ok"] is False
            for removed in ["start_web_server", "get_tailscale_status", "create_terminal_session", "saas_admin"]:
                assert raw_call(removed)["ok"] is False, f"Removed command still enabled: {removed}"
            print("PASS: real FFI, isolated settings/database, command boundaries and event polling")
            selected = Path(directory) / "selected-export.txt"
            adjacent = Path(str(selected) + ".json")
            adjacent.write_text("do not replace", encoding="utf-8")
            exported = call("save_route_credential_export", {"json_text": "[]", "suggested_file_name": "credentials.json", "path": str(selected)})
            assert Path(exported["path"]) == selected and json.loads(selected.read_text(encoding="utf-8")) == []
            assert adjacent.read_text(encoding="utf-8") == "do not replace"
            print("PASS: selected export path is preserved without changing the overwrite target")

            key = call("get_route_proxy_key", {"platform": "codex"})
            assert isinstance(key, str) and len(key) >= 16
            with socket.socket() as occupied:
                if hasattr(socket, "SO_EXCLUSIVEADDRUSE"):
                    occupied.setsockopt(socket.SOL_SOCKET, socket.SO_EXCLUSIVEADDRUSE, 1)
                occupied.bind(("127.0.0.1", 29527))
                occupied.listen(1)
                result = raw_call("start_route_proxy")
                assert result["ok"] is False, "Port conflict must fail, not silently select a different port"
                assert call("get_route_proxy_status")["running"] is False
            print("PASS: occupied 29527 reports failure without switching ports")

            status = call("start_route_proxy")
            assert status["running"] is True and status["port"] == 29527
            assert status["base_url"] == "http://127.0.0.1:29527"
            assert call("plugin_http_port")["port"] == 29527
            preflight = urllib.request.Request(
                "http://127.0.0.1:29527/v1/chat/completions", method="OPTIONS",
                headers={"Origin": "http://localhost:3000", "Access-Control-Request-Method": "POST", "Access-Control-Request-Headers": "authorization,content-type"},
            )
            with opener.open(preflight, timeout=10) as response:
                assert response.status in (200, 204), "Browser preflight was rejected before authentication"
                assert response.headers.get("Access-Control-Allow-Origin"), "Missing CORS permission"
            code, _ = http("/v1/models")
            assert code in (401, 403), f"Missing key accepted: {code}"
            code, _ = http("/v1/models", {"Authorization": "Bearer invalid-smoke-key", "x-ai-switch-platform": "codex"})
            assert code in (401, 403), f"Platform header bypasses key: {code}"
            code, body = http("/v1/models", {"Authorization": f"Bearer {key}"})
            assert code == 200, f"Authenticated model listing failed: {code}"
            assert isinstance(json.loads(body).get("data"), list)
            code, _ = http("/v1/models", {"Authorization": "Bearer codex-login-token", "x-api-key": key})
            assert code == 200, "Codex login Authorization displaced the explicit local key"
            code, _ = http("/v1/models", {"Authorization": f"Bearer {key}", "x-api-key": "invalid-explicit-key"})
            assert code in (401, 403), "An invalid explicit key must not fall back to another identity"
            for path in ["/", "/api/get_settings", "/ws/events", "/api/saas/admin", "/v1/files", "/other/messages"]:
                code, _ = http(path, {"Authorization": f"Bearer {key}"})
                assert code in (404, 405), f"Management/static route is mounted: {path} ({code})"
            print("PASS: AI-only 29527 listener, local API-key authentication and rejected management routes")
            check_proxy_forwarding(call, opener, key)

            assert call("stop_route_proxy")["running"] is False
            assert_port_released()
            assert call("start_route_proxy")["running"] is True
            shutdown_started = time.monotonic()
            call("__otools_shutdown__")
            assert time.monotonic() - shutdown_started < 3, "Idle plugin shutdown exceeded the host's 3-second budget"
            assert_port_released()
            assert raw_call("get_settings")["ok"] is False, "A retired library must reject late business calls"
            call("__otools_initialize__")
            assert call("get_route_proxy_key", {"platform": "codex"}) == key
            restored = call("get_route_proxy_status")
            if restored["running"]:
                assert restored["port"] == 29527, "Restored service changed its configured port"
                call("stop_route_proxy")
            call("__otools_shutdown__")
            assert any(Path(directory).rglob("*.db")), "Database was not created in isolated data directory"
            print("PASS: stop/restart/shutdown releases listener, reinitialization retains isolated state")
        finally:
            call("__otools_shutdown__")


if __name__ == "__main__":
    main()
