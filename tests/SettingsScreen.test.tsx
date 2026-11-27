import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactElement } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  disableRouteProxyHttps,
  deleteRouteProxyHttpsCertificates,
  enableRouteProxyHttps,
  getRouteProxyHttpsStatus,
  getRouteProxyStatus,
  getSettings,
  getUsageHistoryStorage,
  getRoutePool,
  setRoutePoolBaselineModels,
  listConfigSnapshots,
  openRouteProxyHttpsCertificateDirectory,
  regenerateRouteProxyHttpsCertificates,
  reimportRouteProxyRootCa,
  saveSettings,
  testNotification,
  uninstallRouteProxyRootCa,
  pluginPackList,
  pluginPackGetDetail,
  pluginPackInstallFolder,
  pluginPackInstallUrl,
  pluginPackSetEnabled,
  pluginPackUninstall,
} from "../src/lib/api/client";
import { I18nProvider } from "../src/lib/i18n";
import { createQueryClient } from "../src/lib/query/queryClient";
import { SettingsScreen } from "../src/screens/SettingsScreen";
import { createDefaultModalityVisibility } from "../src/lib/accountModalities";
import { settingsFixture } from "../src/test/fixtures";

vi.mock("../src/lib/api/client", () => ({
  getSettings: vi.fn(),
  getRoutePool: vi.fn(),
  setRoutePoolBaselineModels: vi.fn(),
  saveSettings: vi.fn(),
  getRouteProxyStatus: vi.fn(),
  getRouteProxyHttpsStatus: vi.fn(),
  enableRouteProxyHttps: vi.fn(),
  disableRouteProxyHttps: vi.fn(),
  reimportRouteProxyRootCa: vi.fn(),
  regenerateRouteProxyHttpsCertificates: vi.fn(),
  uninstallRouteProxyRootCa: vi.fn(),
  deleteRouteProxyHttpsCertificates: vi.fn(),
  openRouteProxyHttpsCertificateDirectory: vi.fn(),
  getUsageHistoryStorage: vi.fn(),
  compactUsageHistory: vi.fn(),
  listConfigSnapshots: vi.fn(),
  testNotification: vi.fn(),
  getPiiModelStatus: vi.fn(),
  startPiiModelDownload: vi.fn(),
  cancelPiiModelDownload: vi.fn(),
  deletePiiModel: vi.fn(),
  warmPiiModel: vi.fn(),
  pluginPackList: vi.fn(),
  pluginPackGetDetail: vi.fn(),
  pluginPackInstallFolder: vi.fn(),
  pluginPackInstallUrl: vi.fn(),
  pluginPackSetEnabled: vi.fn(),
  pluginPackUninstall: vi.fn(),
}));
vi.mock("../src/lib/transport", () => ({
  isDesktop: vi.fn(() => true),
  getTransport: vi.fn(() => ({ subscribe: vi.fn(() => Promise.resolve(vi.fn())) })),
}));

const httpsStatusFixture = {
  enabled: false,
  certReady: false,
  trustStatus: "untrusted" as const,
  trustAdapter: null,
  rootFingerprint: null,
  expiresAt: null,
  certificateDir: "C:/Users/example/.ai-switch/certs/route-proxy",
  rootCertificatePath: null,
  proxyBaseUrl: null,
  message: null,
  manualInstructions: [] as string[],
};

const httpsOutcomeFixture = {
  https: {
    ...httpsStatusFixture,
    enabled: true,
    certReady: true,
    trustStatus: "systemTrusted" as const,
    rootFingerprint: "a".repeat(64),
    expiresAt: "2027-07-25T00:00:00Z",
    proxyBaseUrl: "https://127.0.0.1:19528",
  },
  routeProxy: {
    running: true,
    bind_host: "127.0.0.1",
    port: 19527,
    base_url: "http://127.0.0.1:19527",
    https_port: 19528,
    https_base_url: "https://127.0.0.1:19528",
    https_error: null,
  },
  configWrites: [],
};

function renderSettings(extra: ReactElement | null = null) {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <I18nProvider initialLanguage="zh-CN">
        {extra ?? <SettingsScreen />}
      </I18nProvider>
    </QueryClientProvider>,
  );
}

async function openTab(label: string) {
  await userEvent.click(await screen.findByRole("tab", { name: label }));
}

describe("SettingsScreen", () => {
  beforeEach(() => {
    window.localStorage.clear();
    vi.mocked(getSettings).mockReset();
    vi.mocked(getSettings).mockResolvedValue(settingsFixture);
    vi.mocked(saveSettings).mockReset();
    vi.mocked(saveSettings).mockImplementation(async (settings) => settings);
    vi.mocked(getRoutePool).mockReset();
    vi.mocked(getRoutePool).mockResolvedValue({
      platform: "codex",
      baseline_models: ["gpt-6-astra", "gpt-5.6-sol"],
      default_baseline_models: ["gpt-6-astra", "gpt-5.6-sol"],
    } as never);
    vi.mocked(setRoutePoolBaselineModels).mockReset();
    vi.mocked(setRoutePoolBaselineModels).mockImplementation(async ({ models }) => ({
      platform: "codex", baseline_models: models,
      default_baseline_models: ["gpt-6-astra", "gpt-5.6-sol"],
    } as never));
    vi.mocked(getRouteProxyStatus).mockReset();
    vi.mocked(getRouteProxyStatus).mockResolvedValue({
      running: false, bind_host: "127.0.0.1", port: null, base_url: null,
    });
    vi.mocked(getRouteProxyHttpsStatus).mockReset();
    vi.mocked(getRouteProxyHttpsStatus).mockResolvedValue(httpsStatusFixture);
    vi.mocked(enableRouteProxyHttps).mockReset();
    vi.mocked(enableRouteProxyHttps).mockResolvedValue(httpsOutcomeFixture);
    vi.mocked(disableRouteProxyHttps).mockReset();
    vi.mocked(disableRouteProxyHttps).mockResolvedValue(httpsOutcomeFixture);
    vi.mocked(reimportRouteProxyRootCa).mockReset();
    vi.mocked(reimportRouteProxyRootCa).mockResolvedValue(httpsOutcomeFixture);
    vi.mocked(regenerateRouteProxyHttpsCertificates).mockReset();
    vi.mocked(regenerateRouteProxyHttpsCertificates).mockResolvedValue(httpsOutcomeFixture);
    vi.mocked(uninstallRouteProxyRootCa).mockReset();
    vi.mocked(uninstallRouteProxyRootCa).mockResolvedValue(httpsOutcomeFixture);
    vi.mocked(deleteRouteProxyHttpsCertificates).mockReset();
    vi.mocked(deleteRouteProxyHttpsCertificates).mockResolvedValue(httpsStatusFixture);
    vi.mocked(openRouteProxyHttpsCertificateDirectory).mockReset();
    vi.mocked(openRouteProxyHttpsCertificateDirectory).mockResolvedValue();
    vi.mocked(getUsageHistoryStorage).mockReset();
    vi.mocked(getUsageHistoryStorage).mockResolvedValue({ reclaimable_bytes: 0 });
    vi.mocked(listConfigSnapshots).mockReset();
    vi.mocked(listConfigSnapshots).mockResolvedValue([]);
    vi.mocked(testNotification).mockReset();
    vi.mocked(pluginPackList).mockReset();
    vi.mocked(pluginPackList).mockResolvedValue([]);
    vi.mocked(pluginPackGetDetail).mockReset();
    vi.mocked(pluginPackInstallFolder).mockReset();
    vi.mocked(pluginPackInstallUrl).mockReset();
    vi.mocked(pluginPackSetEnabled).mockReset();
    vi.mocked(pluginPackUninstall).mockReset();
  });

  it("shows only the active tab content", async () => {
    renderSettings();
    expect(await screen.findByRole("combobox", { name: "主题外观" })).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "代理地址" })).not.toBeInTheDocument();
    await openTab("代理");
    expect(await screen.findByRole("textbox", { name: "代理地址" })).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.queryByRole("combobox", { name: "主题外观" })).not.toBeInTheDocument(),
    );
  });

  it("opens the plugins tab and shows the pack manager", async () => {
    renderSettings();
    await openTab("插件");
    expect(await screen.findByRole("button", { name: "从文件夹安装" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "从 URL 安装" })).toBeInTheDocument();
    expect(screen.getByText("尚未安装插件包。")).toBeInTheDocument();
    await waitFor(() =>
      expect(screen.queryByRole("combobox", { name: "主题外观" })).not.toBeInTheDocument(),
    );
  });

  it("opens the privacy tab and shows masking controls (regression)", async () => {
    renderSettings();
    await openTab("隐私脱敏");
    expect(await screen.findByRole("checkbox", { name: "隐私脱敏" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "隐私脱敏规则 JSON" })).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "启用本地 PII 模型" })).toBeInTheDocument();
  });

  it("opens Insights from the privacy tab", async () => {
    const onOpenFeature = vi.fn();
    renderSettings(<SettingsScreen onOpenFeature={onOpenFeature} />);
    await openTab("隐私脱敏");
    await userEvent.click(await screen.findByRole("button", { name: "脱敏审计" }));
    expect(onOpenFeature).toHaveBeenCalledWith("Insights");
  });

  it("lets users change which account modalities appear in navigation", async () => {
    const onModalityVisibilityChange = vi.fn();
    renderSettings(
      <SettingsScreen
        modalityVisibility={createDefaultModalityVisibility()}
        onModalityVisibilityChange={onModalityVisibilityChange}
      />,
    );
    await openTab("智能体");
    const videoToggle = await screen.findByRole("checkbox", { name: "显示 视频" });
    expect(videoToggle).toBeChecked();
    expect(screen.queryByRole("checkbox", { name: "显示 Claude" })).not.toBeInTheDocument();
    await userEvent.click(videoToggle);
    expect(onModalityVisibilityChange).toHaveBeenCalledWith("video", false);
  });

  it("edits Codex baseline models from the agents tab", async () => {
    renderSettings();
    await openTab("智能体");
    await userEvent.click(await screen.findByRole("button", { name: "配置 Codex 基线模型" }));
    expect(await screen.findByRole("dialog", { name: "Codex 基线模型" })).toBeInTheDocument();
    expect(screen.getByLabelText("基线模型 1")).toHaveValue("gpt-6-astra");

    await userEvent.click(screen.getByRole("button", { name: "添加模型" }));
    await userEvent.type(screen.getByLabelText("基线模型 3"), "custom-x");
    await userEvent.click(screen.getByRole("button", { name: "保存" }));

    await waitFor(() =>
      expect(setRoutePoolBaselineModels).toHaveBeenCalledWith({
        platform: "codex",
        models: ["gpt-6-astra", "gpt-5.6-sol", "custom-x"],
      }),
    );
  });

  it("saves theme and language from the general tab", async () => {
    renderSettings();
    const themeSelect = await screen.findByRole("combobox", { name: "主题外观" });
    await userEvent.selectOptions(themeSelect, "dark");
    await waitFor(() => expect(vi.mocked(saveSettings).mock.calls.at(-1)?.[0]).toEqual(
      expect.objectContaining({ theme: "dark" }),
    ));
    expect(await screen.findByText("设置已保存。")).toBeInTheDocument();

    const languageSelect = screen.getByRole("combobox", { name: "语言" });
    await userEvent.selectOptions(languageSelect, "zh-CN");
    await waitFor(() => expect(vi.mocked(saveSettings).mock.calls.at(-1)?.[0]).toEqual(
      expect.objectContaining({ language: "zh-CN" }),
    ));
  });

  it("defaults the request body limit to 128 MiB and saves custom values", async () => {
    renderSettings();
    await openTab("数据");
    const input = await screen.findByRole("spinbutton", { name: "模型代理请求体上限（MiB）" });
    expect(input).toHaveValue(128);
    for (const limit of [256, 0]) {
      await userEvent.clear(input);
      await userEvent.type(input, String(limit));
      await userEvent.tab();
      await waitFor(() => expect(vi.mocked(saveSettings).mock.calls.at(-1)?.[0]).toEqual({
        ...settingsFixture, route_proxy_request_body_limit_mib: limit,
      }));
    }
  });

  it.each(["", "-1", "1.5", "4294967296"])("rejects invalid request body limit %s", async (value) => {
    renderSettings();
    await openTab("数据");
    const input = await screen.findByRole("spinbutton", { name: "模型代理请求体上限（MiB）" });
    await userEvent.clear(input);
    if (value) await userEvent.type(input, value);
    await userEvent.tab();
    expect(await screen.findByText("请输入有效的非负整数，0 表示不限。")).toBeInTheDocument();
    expect(saveSettings).not.toHaveBeenCalled();
  });

  it("validates and saves proxy settings from the proxy tab", async () => {
    renderSettings();
    await openTab("代理");
    const enableToggle = await screen.findByRole("checkbox", { name: "启用代理" });
    const addressInput = screen.getByRole("textbox", { name: "代理地址" });
    await userEvent.click(enableToggle);
    expect(await screen.findByText("启用代理时必须填写代理地址。")).toBeInTheDocument();
    expect(saveSettings).not.toHaveBeenCalled();
    await userEvent.type(addressInput, "http://127.0.0.1:7890 ");
    await userEvent.click(enableToggle);
    await userEvent.tab();
    await waitFor(() => expect(vi.mocked(saveSettings).mock.calls.at(-1)?.[0]).toEqual({
      ...settingsFixture, proxy_enabled: true, proxy_url: "http://127.0.0.1:7890",
    }));
  });

  it("opens HTTPS settings from the network tab", async () => {
    renderSettings();
    await openTab("网络");
    expect(await screen.findByRole("heading", { name: "HTTPS" })).toBeInTheDocument();
    expect(screen.getByText("本地算力池 HTTPS")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("checkbox", { name: "为本地算力池启用 HTTPS" }));
    await waitFor(() => expect(enableRouteProxyHttps).toHaveBeenCalledTimes(1));
    expect(await screen.findByText("https://127.0.0.1:19528")).toBeInTheDocument();
    expect(screen.getByText("http://127.0.0.1:19527")).toBeInTheDocument();
  });

  it("shows untrusted guidance without blocking HTTPS controls", async () => {
    vi.mocked(getRouteProxyHttpsStatus).mockResolvedValue({
      ...httpsStatusFixture,
      enabled: true,
      certReady: true,
      manualInstructions: ["certutil.exe -user -addstore Root C:/tmp/root-ca.pem"],
    });
    renderSettings();
    await openTab("网络");
    expect(await screen.findByText("根证书尚未受信任")).toBeInTheDocument();
    expect(screen.getByText("certutil.exe -user -addstore Root C:/tmp/root-ca.pem")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "重新导入根证书" })).toBeEnabled();
  });

  it("still surfaces recovery commands when trust could not be verified", async () => {
    vi.mocked(getRouteProxyHttpsStatus).mockResolvedValue({
      ...httpsStatusFixture,
      enabled: true,
      certReady: true,
      trustStatus: "unknown" as const,
      message: "Root CA installation completed, but the local trust store could not verify it",
      manualInstructions: [
        "security add-trusted-cert -r trustRoot -k /Users/example/Library/Keychains/login.keychain-db /tmp/root-ca.pem",
      ],
    });
    renderSettings();
    await openTab("网络");
    expect(
      await screen.findByText(
        "security add-trusted-cert -r trustRoot -k /Users/example/Library/Keychains/login.keychain-db /tmp/root-ca.pem",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Root CA installation completed, but the local trust store could not verify it"),
    ).toBeInTheDocument();
  });

  it("does not expose the removed Codex login entry", async () => {
    renderSettings();
    expect(await screen.findByRole("combobox", { name: "主题外观" })).toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: "保持 Codex 登录态" })).not.toBeInTheDocument();
  });
});
