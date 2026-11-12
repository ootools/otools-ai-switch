import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getTransport } from "../../src/lib/transport";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

describe("OTools management transport", () => {
  beforeEach(() => vi.resetAllMocks());

  it("invokes the host without requiring Tauri globals at module load", async () => {
    vi.mocked(invoke).mockResolvedValue({ running: true, port: 29527 });
    await expect(getTransport().call("start_route_proxy", { port: 29527 })).resolves.toEqual({ running: true, port: 29527 });
    expect(invoke).toHaveBeenCalledWith("start_route_proxy", { port: 29527 });
  });

  it("unwraps the native envelope when the host forwards it", async () => {
    vi.mocked(invoke).mockResolvedValue({ ok: true, data: [{ id: "account-1" }] });
    await expect(getTransport().call("list_route_credentials")).resolves.toEqual([{ id: "account-1" }]);
  });

  it("preserves an ordinary business result with an ok field", async () => {
    const result = { ok: true, count: 4 };
    vi.mocked(invoke).mockResolvedValue(result);
    await expect(getTransport().call("test_route_pool_model")).resolves.toEqual(result);
  });

  it.each([false, true])("preserves structured native errors (envelope=%s)", async (envelope) => {
    const error = { code: "proxy.port_in_use", message: "Port 29527 is in use", recoverable: true, details: "29527", operation_id: "op-1" };
    if (envelope) vi.mocked(invoke).mockResolvedValue({ ok: false, error });
    else vi.mocked(invoke).mockRejectedValue(error);
    await expect(getTransport().call("start_route_proxy")).rejects.toMatchObject({
      name: "ApiClientError", message: error.message, code: error.code, details: "29527", operationId: "op-1",
    });
  });

  it.each([
    ["mcp_scan_local", "mcp.config_invalid"],
    ["skills_list", "skills.read_only"],
    ["write_route_proxy_configs", "config.invalid_json"],
  ])("preserves serialized native errors wrapped by the host for %s", async (command, code) => {
    vi.mocked(invoke).mockImplementation(async () => {
      throw new Error(JSON.stringify({
        code,
        message: "The host configuration is invalid",
        details: "D:/host/config.json: expected an object",
        recoverable: false,
        operation_id: "op-host-42",
      }));
    });

    await expect(getTransport().call(command)).rejects.toMatchObject({
      name: "ApiClientError",
      message: "The host configuration is invalid",
      code,
      details: "D:/host/config.json: expected an object",
      recoverable: false,
      operationId: "op-host-42",
    });
  });

  it.each(["Host bridge disconnected", "{invalid JSON", "null", "42", "[]"])(
    "preserves the ordinary Error message %s",
    async (message) => {
      vi.mocked(invoke).mockImplementation(async () => {
        throw new Error(message);
      });

      await expect(getTransport().call("get_settings")).rejects.toMatchObject({
        name: "ApiClientError",
        message,
        code: "transport.error",
        details: null,
        recoverable: true,
        operationId: null,
      });
    },
  );

  it("delivers event payloads and releases the host subscription", async () => {
    let deliver: ((event: { event: string; id: number; payload: unknown }) => void) | undefined;
    const unlisten = vi.fn();
    vi.mocked(listen).mockImplementation(async (_topic, handler) => {
      deliver = handler;
      return unlisten;
    });
    const received: unknown[] = [];
    const dispose = await getTransport().subscribe("route-proxy-log", (payload) => received.push(payload));
    deliver?.({ event: "route-proxy-log", id: 1, payload: { status: 200 } });
    expect(received).toEqual([{ status: 200 }]);
    await dispose();
    expect(unlisten).toHaveBeenCalledOnce();
  });
});
