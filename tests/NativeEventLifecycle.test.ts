import { afterEach, beforeEach, expect, it, vi } from "vitest";

beforeEach(() => vi.resetModules());
afterEach(() => {
  delete window.otools;
  Reflect.deleteProperty(window, "__TAURI_EVENT_PLUGIN_INTERNALS__");
});

it("does not discard a newly acquired subscription while an idle release is pending", async () => {
  let resolveSubscription!: (release: () => Promise<void>) => void;
  const firstSubscription = new Promise<() => Promise<void>>((resolve) => { resolveSubscription = resolve; });
  const firstRelease = vi.fn(async () => {});
  const secondRelease = vi.fn(async () => {});
  const listenNative = vi.fn().mockReturnValueOnce(firstSubscription).mockResolvedValue(secondRelease);
  window.otools = { listenNative } as unknown as typeof window.otools;
  const { attachTransformListener, detachTransformListener, registerTransformCallback } = await import("../vendor/otools-plugin-sdk/src/native-event-bridge");

  const first = attachTransformListener("first", registerTransformCallback(() => {}));
  const releaseFirst = detachTransformListener(1);
  const second = attachTransformListener("second", registerTransformCallback(() => {}));
  resolveSubscription(firstRelease);
  await Promise.all([first, second, releaseFirst]);
  await detachTransformListener(await second);

  expect(firstRelease).toHaveBeenCalledOnce();
  expect(secondRelease).toHaveBeenCalledOnce();
  expect(listenNative).toHaveBeenCalledTimes(2);
});
