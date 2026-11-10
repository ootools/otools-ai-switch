import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { normalizeApiError } from "../api/errors";
import type { Transport, Unsubscribe } from "./types";

export type { Transport, Unsubscribe };

const subscriptions = new Set<Unsubscribe>();
const pendingCleanup = new Set<Promise<unknown>>();
let closing = false;
let closePromise: Promise<void> | null = null;
let liveLogQueue: Promise<unknown> = Promise.resolve();
let liveLogViewers = 0;

function trackCleanup<T>(promise: Promise<T>): Promise<T> {
  pendingCleanup.add(promise);
  void promise.then(
    () => pendingCleanup.delete(promise),
    () => pendingCleanup.delete(promise),
  );
  return promise;
}

async function callNative<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    const result = await invoke<T>(command, args ?? {});
    // Desktop hosts unwrap native replies; a remote host may forward the
    // envelope. Do not interpret an ordinary business object's `ok` field.
    if (result && typeof result === "object" && "ok" in result) {
      if (result.ok === true && "data" in result) return result.data as T;
      if (result.ok === false && "error" in result) throw result.error;
    }
    return result;
  } catch (error) {
    throw normalizeApiError(error);
  }
}

const transport: Transport = {
  call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    const acquire = command === "subscribe_route_proxy_live_log";
    if (acquire || command === "unsubscribe_route_proxy_live_log") {
      if (acquire && closing) return Promise.reject(new Error("Panel transport is closed."));
      // A StrictMode cleanup or panel close can arrive while the acquire is
      // pending. Serialize viewer ref changes so release can never overtake it.
      const operation = liveLogQueue.then(async () => {
        if (!acquire && liveLogViewers === 0) return undefined as T;
        const result = await callNative<T>(command, args);
        liveLogViewers += acquire ? 1 : -1;
        return result;
      });
      liveLogQueue = operation.catch(() => undefined);
      return trackCleanup(operation);
    }
    return callNative<T>(command, args);
  },
  subscribe<T>(event: string, handler: (payload: T) => void): Promise<Unsubscribe> {
    if (closing) return Promise.reject(new Error("Panel transport is closed."));
    const subscription = listen<T>(event, (message) => {
      if (!closing) handler(message.payload);
    }).then(async (unlisten) => {
      let releasePromise: Promise<void> | null = null;
      const release = () => {
        if (!releasePromise) {
          subscriptions.delete(release);
          releasePromise = trackCleanup(Promise.resolve().then(unlisten));
        }
        return releasePromise;
      };
      if (closing) await release();
      else subscriptions.add(release);
      return release;
    });
    return trackCleanup(subscription);
  },
  isDesktop: () => true,
};

export function getTransport(): Transport {
  return transport;
}

/** Release this view's subscriptions, never the persistent native AI service. */
export function closeTransport(): Promise<void> {
  if (closePromise) return closePromise;
  closing = true;
  closePromise = (async () => {
    for (const release of subscriptions) void release();
    // Pending attaches may add their own asynchronous release after unmount.
    while (pendingCleanup.size) {
      await Promise.allSettled([...pendingCleanup]);
    }
    // Covers an acquired live-log ref whose React cleanup did not run.
    while (liveLogViewers > 0) {
      await transport.call("unsubscribe_route_proxy_live_log");
    }
  })();
  return closePromise;
}

/** Host closes a registered panel only after its token is acknowledged. */
export async function registerPanelCloseHook(root: { unmount(): void }): Promise<void> {
  let teardown: Promise<void> | null = null;
  const acknowledgedTokens = new Set<string>();
  const cleanup = () => {
    teardown ??= Promise.resolve().then(async () => {
      try {
        root.unmount();
      } finally {
        await closeTransport();
      }
    });
    return teardown;
  };
  const releaseHook = await transport.subscribe<{ token?: string }>(
    "otools:tab-before-close",
    (payload) => {
      const token = payload?.token;
      if (!token || acknowledgedTokens.has(token)) return;
      acknowledgedTokens.add(token);
      void (async () => {
        try {
          await cleanup();
        } catch (error) {
          console.error("Panel subscription cleanup failed", error);
        } finally {
          await callNative("resolve_tools_tab_close_ack", { token });
        }
      })().catch((error) => console.error("Panel close acknowledgement failed", error));
    },
  );
  try {
    await callNative("register_tools_tab_close_hook", { enabled: true });
  } catch (error) {
    await releaseHook();
    throw error;
  }
  // Best effort for browser navigation, where there is no host ack handshake.
  window.addEventListener("pagehide", () => {
    void cleanup().catch((error) => console.error("Panel subscription cleanup failed", error));
  }, { once: true });
}

// Local file operations always target the OTools host, including remote views.
// The SDK owns bridge readiness and platform selection; never inspect Tauri globals.
export function isDesktop(): boolean {
  return true;
}
