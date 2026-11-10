import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { fetchRouteProxyModels } from "../src/lib/routeProxyModels";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("host proxy model list", () => {
  beforeEach(() => vi.resetAllMocks());

  it("reads the host catalog without a browser request to loopback", async () => {
    const models = [{ id: "gpt-6-astra", owned_by: "ai-switch", context_window: 1050000 }];
    vi.mocked(invoke).mockResolvedValue(models);
    await expect(fetchRouteProxyModels("codex")).resolves.toEqual(models);
    expect(invoke).toHaveBeenCalledWith("list_route_proxy_models", { platform: "codex" });
  });

  it("reports a stopped listener without requesting an API key from the browser", async () => {
    vi.mocked(invoke).mockRejectedValue({ code: "proxy.not_running", message: "Start the AI proxy first" });
    await expect(fetchRouteProxyModels("gemini")).rejects.toMatchObject({ code: "proxy.not_running", message: "Start the AI proxy first" });
    expect(invoke).toHaveBeenCalledTimes(1);
  });
});
