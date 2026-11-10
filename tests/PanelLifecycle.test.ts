import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../vendor/otools-plugin-sdk/src/runtime")).invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../vendor/otools-plugin-sdk/src/runtime")).listen,
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

function hostFixture() {
  const hostInvoke = vi.fn(async (_command: string, _args?: unknown) => undefined);
  const nativeInvoke = vi.fn(async (_command: string) => undefined as unknown);
  const nativeUnlisten = vi.fn(async () => {});
  const listenNative = vi.fn(async () => nativeUnlisten);
  window.__OToolsEnv = { pluginUuid: "otools-ai-switch" };
  window.__TAURI_INTERNALS__ = { invoke: hostInvoke } as unknown as typeof window.__TAURI_INTERNALS__;
  window.otools = {
    getPluginUuid: () => "otools-ai-switch",
    invokeNative: nativeInvoke,
    invokeNativePlugin: (_uuid: string, command: string) => nativeInvoke(command),
    listenNative,
  } as unknown as typeof window.otools;
  return { hostInvoke, nativeInvoke, nativeUnlisten, listenNative };
}

function requestClose(token: string) {
  window.dispatchEvent(new CustomEvent("otools-tauri:otools:tab-before-close", {
    detail: { payload: { token } },
  }));
}

beforeEach(() => vi.resetModules());
afterEach(() => {
  delete window.otools;
  delete window.__OToolsEnv;
  delete window.__TAURI_INTERNALS__;
  Reflect.deleteProperty(window, "__TAURI_EVENT_PLUGIN_INTERNALS__");
});

describe("panel close lifecycle", () => {
  it("unmounts and waits for native event and live-log releases before acknowledging the host", async () => {
    const { hostInvoke, nativeInvoke, nativeUnlisten } = hostFixture();
    const eventReleased = deferred<void>();
    const logSubscribed = deferred<unknown>();
    const logReleased = deferred<unknown>();
    nativeUnlisten.mockImplementation(() => eventReleased.promise);
    nativeInvoke.mockImplementation(async (command) => {
      if (command === "subscribe_route_proxy_live_log") return logSubscribed.promise;
      if (command === "unsubscribe_route_proxy_live_log") return logReleased.promise;
      throw new Error(`Unexpected native command: ${command}`);
    });
    const lifecycle = await import("../src/lib/transport");
    const transport = lifecycle.getTransport();
    const releaseActivity = await transport.subscribe("route-credential-activity", () => {});
    void transport.call("subscribe_route_proxy_live_log", { platform: "codex" });
    const unmount = vi.fn(() => {
      void releaseActivity();
      void transport.call("unsubscribe_route_proxy_live_log");
    });
    await lifecycle.registerPanelCloseHook({ unmount });
    expect(hostInvoke).toHaveBeenCalledWith("register_tools_tab_close_hook", { enabled: true });

    requestClose("close-1");
    requestClose("close-1");
    await waitFor(() => expect(unmount).toHaveBeenCalledOnce());
    expect(nativeInvoke).not.toHaveBeenCalledWith("unsubscribe_route_proxy_live_log");
    expect(hostInvoke).not.toHaveBeenCalledWith("resolve_tools_tab_close_ack", { token: "close-1" });

    logSubscribed.resolve([]);
    await waitFor(() => expect(nativeInvoke).toHaveBeenCalledWith("unsubscribe_route_proxy_live_log"));
    eventReleased.resolve();
    expect(hostInvoke).not.toHaveBeenCalledWith("resolve_tools_tab_close_ack", { token: "close-1" });
    logReleased.resolve(undefined);
    await waitFor(() => expect(hostInvoke).toHaveBeenCalledWith("resolve_tools_tab_close_ack", { token: "close-1" }));
    expect(hostInvoke.mock.calls.filter(([command]) => command === "resolve_tools_tab_close_ack")).toHaveLength(1);
    expect(nativeUnlisten).toHaveBeenCalledOnce();
    expect(nativeInvoke.mock.calls.map(([command]) => command)).toEqual([
      "subscribe_route_proxy_live_log", "unsubscribe_route_proxy_live_log",
    ]);
  });

  it("releases a native subscription that completes after panel cleanup starts", async () => {
    const { listenNative, nativeUnlisten } = hostFixture();
    const subscribed = deferred<typeof nativeUnlisten>();
    listenNative.mockImplementation(() => subscribed.promise);
    const lifecycle = await import("../src/lib/transport");
    const subscription = lifecycle.getTransport().subscribe("route-proxy-live-log", () => {});
    let closed = false;
    const cleanup = lifecycle.closeTransport().then(() => { closed = true; });
    await Promise.resolve();
    expect(closed).toBe(false);
    subscribed.resolve(nativeUnlisten);
    await cleanup;
    const release = await subscription;
    await release();
    expect(nativeUnlisten).toHaveBeenCalledOnce();
    await expect(lifecycle.getTransport().subscribe("another-topic", () => {})).rejects.toThrow(/closed/i);
  });

  it("does not register a host close hook if its event subscription fails", async () => {
    const { hostInvoke, listenNative } = hostFixture();
    listenNative.mockRejectedValue(new Error("native events unavailable"));
    const lifecycle = await import("../src/lib/transport");
    await expect(lifecycle.registerPanelCloseHook({ unmount: vi.fn() })).rejects.toThrow("native events unavailable");
    expect(hostInvoke).not.toHaveBeenCalled();
  });
});
