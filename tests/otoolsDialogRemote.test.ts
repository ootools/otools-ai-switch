import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { open, save } from "../vendor/otools-plugin-sdk/src/tauri-plugin-dialog-shim";
import type { OToolsAPI, OToolsHostBrowsePayload } from "../vendor/otools-plugin-sdk/src/otools-globals";

const entry = (name: string, kind = "file") => ({ name, path: `/home/user/${name}`, kind, size: 0, mime: "", lastModified: null });
const home: OToolsHostBrowsePayload = {
  currentPath: "/home/user", parentPath: "/home", roots: [],
  entries: [entry("project", "directory"), entry("existing.json"), entry("second.json"), entry("image.png")],
};
let browse: ReturnType<typeof vi.fn>;
let nativeOpen: ReturnType<typeof vi.fn>;
let nativeSave: ReturnType<typeof vi.fn>;
let write: ReturnType<typeof vi.fn>;

beforeEach(() => {
  document.body.replaceChildren();
  browse = vi.fn(async (path: string) => {
    if (!path || path === "/home/user") return home;
    if (path === "/home/user/project") return { currentPath: path, parentPath: "/home/user", roots: [], entries: [] };
    throw new Error("Host directory is unavailable");
  });
  // A remote server-native dialog would either reject Tauri's directory boolean
  // or wait for somebody at the host. Keep the external facade isolated here.
  nativeOpen = vi.fn(async () => null);
  nativeSave = vi.fn(async () => null);
  write = vi.fn();
  Object.assign(window, {
    __OToolsEnv: { runtime: "web", pluginUuid: "otools-ai-switch" },
    otools: { invokeNative: vi.fn(async () => { throw new Error("unexpected native command"); }), browseHostDir: browse, writeHostFile: write, dialog: { open: nativeOpen, save: nativeSave } } as unknown as OToolsAPI,
  });
});
afterEach(() => {
  fireEvent.keyDown(window, { key: "Escape" });
  delete window.otools;
  delete window.__OToolsEnv;
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

async function loaded() {
  await waitFor(() => expect(document.querySelector("[data-host-tree]"), "remote picker must render in the requesting page").not.toBeNull());
  await waitFor(() => expect(screen.getByRole("button", { name: /选择此目录|保存/ })).toBeEnabled());
}

const row = (path: string) => {
  const element = document.querySelector(`[data-host-tree-entry][data-path="${path}"]`);
  expect(element).not.toBeNull();
  return element as HTMLElement;
};

describe("OTools remote dialogs", () => {
  it("picks a host project directory in the page without forwarding directory:true to the remote native dialog", async () => {
    const result = open({ directory: true, defaultPath: "/home/user", title: "Project" });
    await loaded();
    fireEvent.click(row("/home/user/project"));
    await waitFor(() => expect(document.querySelector(".otools-host-tree-path")).toHaveTextContent("/home/user/project"));
    fireEvent.click(screen.getByRole("button", { name: "选择此目录" }));
    await expect(result).resolves.toBe("/home/user/project");
    expect(nativeOpen).not.toHaveBeenCalled();
    expect(write).not.toHaveBeenCalled();
  });

  it("supports filtered multiple host files and returns all selected paths", async () => {
    const result = open({ multiple: true, defaultPath: "/home/user", filters: [{ name: "JSON", extensions: ["json"] }] });
    await waitFor(() => expect(document.querySelector('[data-path="/home/user/existing.json"]')).not.toBeNull());
    expect(document.querySelector('[data-path="/home/user/image.png"]')).toBeNull();
    fireEvent.click(row("/home/user/existing.json"));
    fireEvent.click(row("/home/user/second.json"));
    fireEvent.click(screen.getByRole("button", { name: "选择" }));
    await expect(result).resolves.toEqual(["/home/user/existing.json", "/home/user/second.json"]);
  });

  it("preserves a save defaultPath, accepts a new filename, and only returns a host path", async () => {
    const result = save({ defaultPath: "/home/user/credentials.json", filters: [{ name: "JSON", extensions: ["json"] }] });
    await loaded();
    const input = screen.getByRole("textbox", { name: "文件名" });
    expect(input).toHaveValue("credentials.json");
    fireEvent.input(input, { target: { value: "new-credentials" } });
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    await expect(result).resolves.toBe("/home/user/new-credentials.json");
    expect(nativeSave).not.toHaveBeenCalled();
    expect(write).not.toHaveBeenCalled();
  });

  it("requires local confirmation before returning an existing save target and permits rejecting it", async () => {
    const result = save({ defaultPath: "/home/user/existing.json" });
    let settled = false;
    void result.then(() => { settled = true; });
    await loaded();
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    fireEvent.click(await screen.findByRole("button", { name: "返回" }));
    expect(settled).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "保存" }));
    fireEvent.click(await screen.findByRole("button", { name: "覆盖" }));
    await expect(result).resolves.toBe("/home/user/existing.json");
    expect(write).not.toHaveBeenCalled();
  });

  it("cancels without returning a destination or writing a file", async () => {
    const result = save({ defaultPath: "credentials.json" });
    await loaded();
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    await expect(result).resolves.toBeNull();
    expect(write).not.toHaveBeenCalled();
    expect(nativeSave).not.toHaveBeenCalled();
  });

  it("does not return an invalid target when the host directory cannot be read", async () => {
    const result = save({ defaultPath: "/missing/credentials.json" });
    await waitFor(() => expect(document.querySelector("[data-host-tree]"), "remote picker must render").not.toBeNull());
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Host directory is unavailable"));
    expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
    fireEvent.keyDown(window, { key: "Escape" });
    await expect(result).resolves.toBeNull();
  });

  it("retains native dialogs on desktop", async () => {
    Object.assign(window, { __OToolsEnv: { runtime: "desktop", pluginUuid: "otools-ai-switch" } });
    nativeOpen.mockResolvedValue("D:\\project");
    nativeSave.mockResolvedValue("D:\\credentials.json");
    await expect(open({ directory: true })).resolves.toBe("D:\\project");
    await expect(save({ defaultPath: "credentials.json" })).resolves.toBe("D:\\credentials.json");
    expect(browse).not.toHaveBeenCalled();
    expect(document.querySelector("[data-host-tree]")).toBeNull();
  });
});
