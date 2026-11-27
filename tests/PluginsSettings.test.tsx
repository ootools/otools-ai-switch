import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { open } from "@tauri-apps/plugin-dialog";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  pluginPackGetDetail,
  pluginPackInstallFolder,
  pluginPackInstallUrl,
  pluginPackList,
  pluginPackSetEnabled,
  pluginPackUninstall,
} from "../src/lib/api/client";
import { PluginsSettings } from "../src/components/settings/PluginsSettings";
import { I18nProvider } from "../src/lib/i18n";
import { createQueryClient } from "../src/lib/query/queryClient";
import type { PluginDetail, PluginSummary } from "../src/lib/api/types";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("../src/lib/api/client", () => ({
  pluginPackList: vi.fn(),
  pluginPackGetDetail: vi.fn(),
  pluginPackInstallFolder: vi.fn(),
  pluginPackInstallUrl: vi.fn(),
  pluginPackSetEnabled: vi.fn(),
  pluginPackUninstall: vi.fn(),
}));

const pack: PluginSummary = {
  id: "example-relay",
  version: "1.0.0",
  name: "Example Relay",
  description: "A relay pack",
  author: "tester",
  license: "MIT",
  enabled: true,
  preset_count: 1,
  source: "local",
  warnings: [],
  has_middleware: false,
};

const detail: PluginDetail = {
  manifest: {
    schema_version: 1,
    id: "example-relay",
    version: "1.0.0",
    name: "Example Relay",
    description: "A relay pack",
    author: "tester",
    license: "MIT",
    contributes: {
      provider_presets: [
        {
          id: "primary",
          platform: "codex",
          label: "Example",
          provider: "Example",
          default_name: "Example",
          base_url: "https://example.com/v1",
          interface_format: "openai",
          model_mappings: [{ from: "m1", to: "m1" }],
        },
      ],
    },
  },
  enabled: true,
  source: "local",
  path: "/data/plugins/example-relay",
};

function renderPanel() {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <I18nProvider initialLanguage="zh-CN">
        <PluginsSettings />
      </I18nProvider>
    </QueryClientProvider>,
  );
}

describe("PluginsSettings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(pluginPackList).mockResolvedValue([pack]);
    vi.mocked(pluginPackGetDetail).mockResolvedValue(detail);
    vi.mocked(pluginPackInstallFolder).mockResolvedValue(pack);
    vi.mocked(pluginPackInstallUrl).mockResolvedValue(pack);
    vi.mocked(pluginPackSetEnabled).mockResolvedValue();
    vi.mocked(pluginPackUninstall).mockResolvedValue();
    vi.mocked(open).mockResolvedValue(null);
    window.confirm = vi.fn(() => true);
  });

  it("lists installed packs with their details", async () => {
    renderPanel();
    expect(await screen.findByRole("button", { name: "Example Relay" })).toBeInTheDocument();
    expect(screen.getByText("v1.0.0")).toBeInTheDocument();
    expect(screen.getByText(/1 个预设/)).toBeInTheDocument();
    expect(await screen.findByText("https://example.com/v1")).toBeInTheDocument();
  });

  it("shows an empty state when nothing is installed", async () => {
    vi.mocked(pluginPackList).mockResolvedValue([]);
    renderPanel();
    expect(await screen.findByText("尚未安装插件包。")).toBeInTheDocument();
  });

  it("toggles a pack on and off", async () => {
    renderPanel();
    const toggle = await screen.findByRole("checkbox", { name: "已启用 Example Relay" });
    await userEvent.click(toggle);
    await waitFor(() =>
      expect(pluginPackSetEnabled).toHaveBeenCalledWith("example-relay", false),
    );
  });

  it("uninstalls a pack after confirmation", async () => {
    renderPanel();
    await userEvent.click(
      await screen.findByRole("button", { name: "卸载 Example Relay" }),
    );
    await waitFor(() => expect(pluginPackUninstall).toHaveBeenCalledWith("example-relay"));
  });

  it("installs from a folder picked in the dialog", async () => {
    vi.mocked(open).mockResolvedValue("/tmp/example-relay");
    renderPanel();
    await userEvent.click(
      await screen.findByRole("button", { name: "从文件夹安装" }),
    );
    await waitFor(() =>
      expect(pluginPackInstallFolder).toHaveBeenCalledWith("/tmp/example-relay"),
    );
  });

  it("requires a SHA-256 digest for URL installs", async () => {
    renderPanel();
    const install = await screen.findByRole("button", { name: "安装" });
    expect(install).toBeDisabled();
    await userEvent.type(screen.getByRole("textbox", { name: "从 URL 安装" }), "https://x.test/plugin.json");
    expect(install).toBeDisabled();
    await userEvent.type(
      screen.getByRole("textbox", { name: "SHA-256（64 位十六进制）" }),
      "a".repeat(64),
    );
    await userEvent.click(install);
    await waitFor(() =>
      expect(pluginPackInstallUrl).toHaveBeenCalledWith("https://x.test/plugin.json", "a".repeat(64)),
    );
  });
});
