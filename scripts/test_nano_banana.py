"""Verify both Nano Banana models through the real native image bridge.

Only a local mock upstream and temporary plugin data are used. This does not
call Google's API, consume quota or verify a user's model access permissions.
"""

import base64
import ctypes
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import tempfile
import threading
import unittest
from urllib.parse import parse_qs, urlsplit

ROOT = Path(__file__).resolve().parents[1]
PNG = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/l9sAAAAASUVORK5CYII="
)
MODELS = [
    ("nano-banana", "gemini-2.5-flash-image"),
    ("nano-banana-pro", "gemini-3-pro-image-preview"),
]


class NanoBananaNativeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory(prefix="otools-nano-banana-")
        cls.previous_data_dir = os.environ.get("OTOOLS_AI_SWITCH_DATA_DIR")
        os.environ["OTOOLS_AI_SWITCH_DATA_DIR"] = cls.directory.name
        library = Path(os.environ.get("OTOOLS_AI_SWITCH_TEST_LIBRARY", str(ROOT / "lib/Windows.dll"))).resolve(strict=True)
        cls.native = ctypes.CDLL(str(library))
        cls.native.otools_plugin_invoke.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_size_t)]
        cls.native.otools_plugin_invoke.restype = ctypes.c_void_p
        cls.native.otools_plugin_free.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
        cls.native.otools_plugin_free.restype = None
        cls.captures = []

        class Upstream(BaseHTTPRequestHandler):
            def log_message(self, *unused):
                pass

            def do_POST(self):
                body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                cls.captures.append({"path": self.path, "authorization": self.headers.get("Authorization"), "body": body})
                response = json.dumps({
                    "responseId": "local-nano-image",
                    "candidates": [{"content": {"parts": [
                        {"text": "local image"},
                        {"inlineData": {"mimeType": "image/png", "data": base64.b64encode(PNG).decode()}},
                    ]}}],
                }).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(response)))
                self.end_headers()
                self.wfile.write(response)

        cls.upstream = ThreadingHTTPServer(("127.0.0.1", 0), Upstream)
        cls.worker = threading.Thread(target=cls.upstream.serve_forever, daemon=True)
        cls.worker.start()
        ids = []
        for alias, model in MODELS:
            account = cls.call("create_api_route_credential", {"input": {
                "platform": "codex", "display_name": alias, "api_key": "test-google-image-key",
                "base_url": f"http://127.0.0.1:{cls.upstream.server_port}", "interface_format": "gemini",
                "model_mappings_json": json.dumps([{"from": alias, "to": model, "capabilities": ["image.generate"]}]),
                "preview_json": None, "batch_id": None,
            }})
            ids.append(account["id"])
        cls.call("set_route_pool_members", {"input": {"platform": "codex", "account_ids": ids}})

    @classmethod
    def call(cls, method, payload=None):
        encoded = json.dumps({"method": method, "payload": payload or {}}).encode()
        buffer = ctypes.create_string_buffer(encoded)
        length = ctypes.c_size_t()
        ptr = cls.native.otools_plugin_invoke(buffer, len(encoded), ctypes.byref(length))
        if not ptr:
            raise RuntimeError(f"{method}: null response")
        try:
            result = json.loads(ctypes.string_at(ptr, length.value))
        finally:
            cls.native.otools_plugin_free(ptr, length.value)
        if result.get("ok") is not True:
            raise RuntimeError(f"{method}: {result.get('error')}")
        return result["data"]

    @classmethod
    def tearDownClass(cls):
        try:
            cls.call("__otools_shutdown__")
        finally:
            cls.upstream.shutdown()
            cls.upstream.server_close()
            cls.worker.join(timeout=5)
            cls.directory.cleanup()
            if cls.previous_data_dir is None:
                os.environ.pop("OTOOLS_AI_SWITCH_DATA_DIR", None)
            else:
                os.environ["OTOOLS_AI_SWITCH_DATA_DIR"] = cls.previous_data_dir

    def test_both_models_are_available_as_distinct_image_models(self):
        models = self.call("imagegen_list_models", {"platform": "codex"})
        self.assertEqual({(item["id"], item["upstream_model"]) for item in models}, set(MODELS))
        self.assertTrue(all("image.generate" in item["capabilities"] for item in models))

    def test_each_model_uses_its_mapped_generate_content_endpoint_and_stores_the_image(self):
        for alias, model in MODELS:
            with self.subTest(model=model):
                session = self.call("imagegen_create_session", {"input": {"title": alias, "platform": "codex"}})
                result = self.call("imagegen_generate", {"input": {
                    "session_id": session["id"], "model": alias, "prompt": "local Nano Banana test",
                    "count": 1, "size": "1024x1024", "quality": "auto",
                }})
                request = self.captures[-1]
                endpoint = urlsplit(request["path"])
                self.assertEqual(endpoint.path, f"/v1beta/models/{model}:generateContent")
                self.assertEqual(parse_qs(endpoint.query).get("key"), ["test-google-image-key"])
                self.assertIsNone(request["authorization"])
                self.assertEqual(request["body"]["contents"][0]["parts"][0]["text"], "local Nano Banana test")
                self.assertEqual(request["body"]["generationConfig"]["responseModalities"], ["TEXT", "IMAGE"])
                self.assertNotIn("messages", request["body"])
                self.assertEqual(result["message"]["status"], "completed")
                self.assertEqual(len(result["assets"]), 1)
                asset = self.call("imagegen_read_asset", {"id": result["assets"][0]["id"]})
                self.assertEqual(base64.b64decode(asset["data_base64"]), PNG)
                self.assertEqual(asset["asset"]["mime_type"], "image/png")


if __name__ == "__main__":
    unittest.main()
