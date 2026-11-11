"""Test-only host facade for inspecting the built UI against the real native library.

This script is not part of the plugin package or its runtime architecture.
Only local test data and the limited command allowlist below are available.
"""

import argparse
import ctypes
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import mimetypes
import os
from pathlib import Path
import tempfile
import threading
from urllib.parse import unquote, urlsplit


ALLOW = {
    "get_settings", "save_settings", "list_platform_capabilities", "get_disk_space_status",
    "get_route_proxy_status", "get_route_proxy_https_status", "get_route_proxy_key", "list_route_proxy_models",
    "start_route_proxy", "stop_route_proxy", "get_route_pool", "get_client_direct_modes",
    "list_route_credentials", "list_route_credentials_page", "get_route_credential",
    "create_api_route_credential", "update_route_credential", "delete_route_credential",
    "create_route_pool_group", "update_route_pool_group", "delete_route_pool_group",
    "set_route_pool_group_members", "set_route_pool_members", "set_route_pool_baseline_models",
    "set_route_pool_model_mode", "get_model_price_configs", "get_usage_history_storage",
    "get_usage_overview", "get_session_usage_stats", "poll_events",
    "subscribe_route_proxy_live_log", "unsubscribe_route_proxy_live_log",
    "imagegen_list_sessions", "imagegen_list_models", "imagegen_create_session",
    "imagegen_get_conversation", "imagegen_update_session", "imagegen_delete_session",
    "get_claude_slot_plan", "list_batch_groups", "list_config_snapshots",
}

FACADE = """<script>
(() => {
  const listeners = new Set();
  const raw = async (method, payload) => {
    const response = await fetch('/__test/native', {method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({method,payload})});
    return response.json();
  };
  const invoke = async (method,payload) => {
    const result = await raw(method,payload);
    if (!result.ok) throw result.error;
    return result.data;
  };
  window.otools = {
    isDev:()=>true,isWindows:()=>true,isMacOS:()=>false,isLinux:()=>false,
    getAppName:()=> 'OTools test host',getAppVersion:()=> 'test',getPluginUuid:()=> 'otools-ai-switch',
    invokeNative:invoke,invokeNativePlugin:(_uuid,method,payload)=>invoke(method,payload),invokeNativeRaw:raw,
    listenNative:async(handler)=>{listeners.add(handler);return async()=>listeners.delete(handler)},
    shellOpenExternal:()=>{},shellOpenPath:()=>{},
    getPluginLocalState:async()=>null,getPluginLocalStateValue:async()=>null,
    savePluginLocalState:async()=>{},savePluginLocalStateValue:async()=>{},patchPluginLocalState:async()=>{},
  };
  setInterval(async()=>{
    if (!listeners.size) return;
    try { const batch=await invoke('poll_events',{});for(const item of batch.events||[])for(const handler of listeners)handler({payload:item}); } catch {}
  },300);
})();
</script>"""


def main():
    parser = argparse.ArgumentParser()
    root = Path(__file__).resolve().parents[1]
    parser.add_argument("--library", type=Path, default=root / "lib/Windows.dll")
    parser.add_argument("--dist", type=Path, default=root / "dist")
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument(
        "--fail-once", action="append", default=[],
        choices=["list_route_credentials_page", "start_route_proxy", "stop_route_proxy"],
        help="Inject one explicit test failure before calling the real native method",
    )
    parser.add_argument(
        "--failure-count", type=int, default=1,
        help="Number of injected failures per --fail-once method (2 also covers automatic query retry)",
    )
    args = parser.parse_args()
    if args.failure_count < 1:
        parser.error("--failure-count must be positive")
    dist = args.dist.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="otools-ai-switch-preview-") as directory:
        os.environ["OTOOLS_AI_SWITCH_DATA_DIR"] = directory
        native = ctypes.CDLL(str(args.library.resolve(strict=True)))
        native.otools_plugin_invoke.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
        native.otools_plugin_invoke.restype = ctypes.c_void_p
        native.otools_plugin_free.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
        native.otools_plugin_free.restype = None
        lock = threading.Lock()
        failures = {method: args.failure_count for method in args.fail_once}
        failure_lock = threading.Lock()

        def invoke(method, payload):
            data = json.dumps({"method": method, "payload": payload}).encode()
            buffer = ctypes.create_string_buffer(data)
            size = ctypes.c_size_t()
            with lock:
                ptr = native.otools_plugin_invoke(buffer, len(data), ctypes.byref(size))
                if not ptr:
                    raise RuntimeError("null native response")
                try:
                    return ctypes.string_at(ptr, size.value)
                finally:
                    native.otools_plugin_free(ptr, size.value)

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *unused):
                pass

            def respond(self, status, data, mime):
                self.send_response(status)
                self.send_header("Content-Type", mime)
                self.send_header("Content-Length", str(len(data)))
                self.send_header("Cache-Control", "no-store")
                self.end_headers()
                self.wfile.write(data)

            def do_POST(self):
                if self.path != "/__test/native":
                    return self.respond(404, b"not found", "text/plain")
                length = int(self.headers.get("Content-Length", "0"))
                if length < 0 or length > 1024 * 1024:
                    return self.respond(413, b"too large", "text/plain")
                request = json.loads(self.rfile.read(length))
                method = request.get("method")
                with failure_lock:
                    fail = failures.get(method, 0) > 0
                    if fail:
                        failures[method] -= 1
                if method not in ALLOW:
                    data = json.dumps({"ok": False, "error": f"Test host disallows: {method}"}).encode()
                elif fail:
                    error = {
                        "message": "测试宿主模拟的一次故障，请重试",
                        "code": "test.preview_failure",
                        "details": f"Injected once for {method}; retry calls the real native library",
                        "recoverable": True,
                    }
                    data = json.dumps({"ok": False, "error": json.dumps(error, ensure_ascii=False)}).encode()
                else:
                    data = invoke(method, request.get("payload") or {})
                return self.respond(200, data, "application/json")

            def do_GET(self):
                relative = unquote(urlsplit(self.path).path).lstrip("/") or "index.html"
                file = (dist / relative).resolve()
                if not file.is_relative_to(dist) or not file.is_file():
                    return self.respond(404, b"not found", "text/plain")
                data = file.read_bytes()
                if file.name == "index.html":
                    data = data.replace(b"<head>", ("<head>" + FACADE).encode(), 1)
                mime = mimetypes.guess_type(file.name)[0] or "application/octet-stream"
                if file.suffix == ".js":
                    mime = "text/javascript"
                return self.respond(200, data, mime)

        server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
        print(f"Test-only plugin preview: http://127.0.0.1:{server.server_port}", flush=True)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass
        finally:
            server.server_close()
            invoke("__otools_shutdown__", {})


if __name__ == "__main__":
    main()
