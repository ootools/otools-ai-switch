import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  disableRouteProxyHttps,
  deleteRouteProxyHttpsCertificates,
  enableRouteProxyHttps,
  getRouteProxyHttpsStatus,
  getRouteProxyStatus,
  getCodexLoginState,
  getSettings,
  getUsageHistoryStorage,
  openRouteProxyHttpsCertificateDirectory,
  regenerateRouteProxyHttpsCertificates,
  reimportRouteProxyRootCa,
  saveSettings,
  uninstallRouteProxyRootCa,
} from "../src/lib/api/client";
import { I18nProvider } from "../src/lib/i18n";
import { createQueryClient } from "../src/lib/query/queryClient";
import { SettingsScreen } from "../src/screens/SettingsScreen";
import { createDefaultModalityVisibility } from "../src/lib/accountModalities";
import { settingsFixture } from "../src/test/fixtures";

vi.mock("../src/lib/api/client", () => ({
  getSettings: vi.fn(),
  getCodexLoginState: vi.fn(),
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
}));
vi.mock("../src/lib/transport", () => ({
  isDesktop: vi.fn(() => true),
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
  manualInstructions: [],
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
  // HTTPS takes its own port; HTTP keeps serving on the one clients were told to
  // use, so both endpoints are reported at once.
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

describe("SettingsScreen", () => {
  beforeEach(() => {
    window.localStorage.clear();
    vi.mocked(getSettings).mockReset();
    vi.mocked(getCodexLoginState).mockReset();
    // Most cases never touch the Codex-login card, but the query still fires;
    // a signed-in default keeps those tests focused on their own subject.
    vi.mocked(getCodexLoginState).mockResolvedValue({
      usable: true,
      state: "chatgpt",
      auth_mode: "chatgpt",
    });
    vi.mocked(saveSettings).mockReset();
    vi.mocked(getRouteProxyStatus).mockReset();
    vi.mocked(getRouteProxyHttpsStatus).mockReset();
    vi.mocked(enableRouteProxyHttps).mockReset();
    vi.mocked(disableRouteProxyHttps).mockReset();
    vi.mocked(reimportRouteProxyRootCa).mockReset();
    vi.mocked(regenerateRouteProxyHttpsCertificates).mockReset();
    vi.mocked(uninstallRouteProxyRootCa).mockReset();
    vi.mocked(deleteRouteProxyHttpsCertificates).mockReset();
    vi.mocked(openRouteProxyHttpsCertificateDirectory).mockReset();
    vi.mocked(getUsageHistoryStorage).mockReset();
    vi.mocked(getUsageHistoryStorage).mockResolvedValue({ reclaimable_bytes: 0 });
    vi.mocked(getRouteProxyStatus).mockResolvedValue({
      running: false,
      bind_host: "127.0.0.1",
      port: null,
      base_url: null,
    });
    vi.mocked(getRouteProxyHttpsStatus).mockResolvedValue(httpsStatusFixture);
    vi.mocked(enableRouteProxyHttps).mockResolvedValue(httpsOutcomeFixture);
    vi.mocked(disableRouteProxyHttps).mockResolvedValue(httpsOutcomeFixture);
    vi.mocked(reimportRouteProxyRootCa).mockResolvedValue(httpsOutcomeFixture);
    vi.mocked(regenerateRouteProxyHttpsCertificates).mockResolvedValue(httpsOutcomeFixture);
    vi.mocked(uninstallRouteProxyRootCa).mockResolvedValue(httpsOutcomeFixture);
    vi.mocked(deleteRouteProxyHttpsCertificates).mockResolvedValue(httpsStatusFixture);
    vi.mocked(openRouteProxyHttpsCertificateDirectory).mockResolvedValue();
  });

  it("lets users change which account modalities appear in navigation", async () => {
    vi.mocked(getSettings).mockResolvedValue(settingsFixture);
    const onModalityVisibilityChange = vi.fn();

    render(
      <QueryClientProvider client={createQueryClient()}>
        <I18nProvider initialLanguage="zh-CN">
          <SettingsScreen
            modalityVisibility={createDefaultModalityVisibility()}
            onModalityVisibilityChange={onModalityVisibilityChange}
          />
        </I18nProvider>
      </QueryClientProvider>,
    );

    const videoToggle = await screen.findByRole("checkbox", { name: "显示 视频" });
    expect(videoToggle).toBeChecked();
    expect(screen.queryByRole("checkbox", { name: "显示 Claude" })).not.toBeInTheDocument();
    await userEvent.click(videoToggle);
    expect(onModalityVisibilityChange).toHaveBeenCalledWith("video", false);
  });

  it("loads settings and saves a selected theme value", async () => {
    vi.mocked(getSettings).mockResolvedValue(settingsFixture);
    vi.mocked(saveSettings).mockImplementation(async (settings) => settings);

    render(
      <QueryClientProvider client={createQueryClient()}>
        <I18nProvider initialLanguage="zh-CN">
          <SettingsScreen />
        </I18nProvider>
      </QueryClientProvider>,
    );

    expect(await screen.findByText(`数据目录：${settingsFixture.data_dir}`)).toBeInTheDocument();
    expect(screen.getByText("功能入口")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /会话/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /日志/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /MCP/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /批量/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /实例/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /唤醒任务/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /AI 模型/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /导入/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /目标/ })).not.toBeInTheDocument();

    const themeSelect = screen.getByRole("combobox", { name: "主题外观" });
    expect(themeSelect).toHaveValue(settingsFixture.theme === "dark" || settingsFixture.theme === "light" ? settingsFixture.theme : "system");
    await userEvent.selectOptions(themeSelect, "dark");

    await waitFor(() => expect(saveSettings).toHaveBeenCalled());
    expect(vi.mocked(saveSettings).mock.calls[0][0]).toEqual({
      ...settingsFixture,
      theme: "dark",
    });
    expect(await screen.findByText("设置已保存。")).toBeInTheDocument();
  });

  it("defaults the request body limit to 128 MiB and saves custom or unlimited values", async () => {
    vi.mocked(getSettings).mockResolvedValue(settingsFixture);
    vi.mocked(saveSettings).mockImplementation(async (settings) => settings);
    render(
      <QueryClientProvider client={createQueryClient()}>
        <I18nProvider initialLanguage="zh-CN"><SettingsScreen /></I18nProvider>
      </QueryClientProvider>,
    );
    const input = await screen.findByRole("spinbutton", { name: "模型代理请求体上限（MiB）" });
    expect(input).toHaveValue(128);
    for (const limit of [256, 0]) {
      await userEvent.clear(input);
      await userEvent.type(input, String(limit));
      await userEvent.tab();
      await waitFor(() => expect(vi.mocked(saveSettings).mock.calls.at(-1)?.[0]).toEqual({
        ...settingsFixture, route_proxy_request_body_limit_mib: limit,
      }));
      expect(input).toHaveValue(limit);
    }
  });

  it.each(["", "-1", "1.5", "4294967296"])("rejects invalid request body limit %s without saving", async (value) => {
    vi.mocked(getSettings).mockResolvedValue(settingsFixture);
    render(
      <QueryClientProvider client={createQueryClient()}>
        <I18nProvider initialLanguage="zh-CN"><SettingsScreen /></I18nProvider>
      </QueryClientProvider>,
    );
    const input = await screen.findByRole("spinbutton", { name: "模型代理请求体上限（MiB）" });
    await userEvent.clear(input);
    if (value) await userEvent.type(input, value);
    await userEvent.tab();
    expect(await screen.findByText("请输入有效的非负整数，0 表示不限。" )).toBeInTheDocument();
    expect(saveSettings).not.toHaveBeenCalled();
  });

  it("validates and saves proxy settings", async () => {
    vi.mocked(getSettings).mockResolvedValue(settingsFixture);
    vi.mocked(saveSettings).mockImplementation(async (settings) => settings);

    render(
      <QueryClientProvider client={createQueryClient()}>
        <I18nProvider initialLanguage="zh-CN">
          <SettingsScreen />
        </I18nProvider>
      </QueryClientProvider>,
    );

    const enableToggle = await screen.findByRole("checkbox", { name: "启用代理" });
    const addressInput = screen.getByRole("textbox", { name: "代理地址" });

    // Enabling without an address is rejected client-side and nothing saves.
    await userEvent.click(enableToggle);
    expect(await screen.findByText("启用代理时必须填写代理地址。")).toBeInTheDocument();
    expect(saveSettings).not.toHaveBeenCalled();

    // A filled address enables the proxy and persists the trimmed URL.
    await userEvent.type(addressInput, "http://127.0.0.1:7890 ");
    await userEvent.click(enableToggle);
    await userEvent.tab();

    await waitFor(() => expect(saveSettings).toHaveBeenCalled());
    expect(vi.mocked(saveSettings).mock.calls.at(-1)![0]).toEqual({
      ...settingsFixture,
      proxy_enabled: true,
      proxy_url: "http://127.0.0.1:7890",
    });
    expect(await screen.findByText("设置已保存。")).toBeInTheDocument();
  });

  it("leaves the Codex login opt-in and saves it when ticked", async () => {
    vi.mocked(getSettings).mockResolvedValue(settingsFixture);
    vi.mocked(saveSettings).mockImplementation(async (settings) => settings);

    render(
      <QueryClientProvider client={createQueryClient()}>
        <I18nProvider initialLanguage="zh-CN">
          <SettingsScreen />
        </I18nProvider>
      </QueryClientProvider>,
    );

    // Off unless the user opts in: enabling it drops the proxy key from the
    // Codex provider, which only works when a ChatGPT login exists.
    const toggle = await screen.findByRole("checkbox", { name: "保持 Codex 登录态" });
    expect(toggle).not.toBeChecked();

    await userEvent.click(toggle);

    await waitFor(() => expect(saveSettings).toHaveBeenCalled());
    expect(vi.mocked(saveSettings).mock.calls.at(-1)![0]).toEqual({
      ...settingsFixture,
      codex_keep_login_state: true,
    });
  });

  it("warns about a missing ChatGPT login before the switch is used", async () => {
    // Turning the switch on without a ChatGPT login leaves Codex unable to
    // authenticate; the write is refused, so say why before the user tries.
    vi.mocked(getSettings).mockResolvedValue(settingsFixture);
    vi.mocked(getCodexLoginState).mockResolvedValue({
      usable: false,
      state: "not_chatgpt",
      auth_mode: "apikey",
    });

    render(
      <QueryClientProvider client={createQueryClient()}>
        <I18nProvider initialLanguage="zh-CN">
          <SettingsScreen />
        </I18nProvider>
      </QueryClientProvider>,
    );

    expect(
      await screen.findByText(/当前 Codex 只有 API Key 登录，没有可保留的 ChatGPT 登录态/),
    ).toBeInTheDocument();
  });

  it("stays quiet when a ChatGPT login is available", async () => {
    vi.mocked(getSettings).mockResolvedValue(settingsFixture);

    render(
      <QueryClientProvider client={createQueryClient()}>
        <I18nProvider initialLanguage="zh-CN">
          <SettingsScreen />
        </I18nProvider>
      </QueryClientProvider>,
    );

    await screen.findByRole("checkbox", { name: "保持 Codex 登录态" });
    expect(
      screen.queryByText(/没有可保留的 ChatGPT 登录态/),
    ).not.toBeInTheDocument();
  });

  it("saves language changes and updates the selector", async () => {
    const englishSettings = { ...settingsFixture, language: "en" };
    vi.mocked(getSettings).mockResolvedValue(englishSettings);
    vi.mocked(saveSettings).mockImplementation(async (settings) => settings);

    render(
      <QueryClientProvider client={createQueryClient()}>
        <I18nProvider initialLanguage="en">
          <SettingsScreen />
        </I18nProvider>
      </QueryClientProvider>,
    );

    const selector = await screen.findByRole("combobox", { name: "Language" });
    await userEvent.selectOptions(selector, "zh-CN");

    await waitFor(() =>
      expect(vi.mocked(saveSettings).mock.calls[0][0]).toEqual({
        ...englishSettings,
        language: "zh-CN",
      }),
    );
    expect(selector).toHaveValue("zh-CN");
  });


  it("opens feature entries through the settings hub", async () => {
    const onOpenFeature = vi.fn();
    vi.mocked(getSettings).mockResolvedValue(settingsFixture);

    render(
      <QueryClientProvider client={createQueryClient()}>
        <I18nProvider initialLanguage="zh-CN">
          <SettingsScreen onOpenFeature={onOpenFeature} />
        </I18nProvider>
      </QueryClientProvider>,
    );

    await userEvent.click(await screen.findByRole("button", { name: /会话/ }));
    expect(onOpenFeature).toHaveBeenCalledWith("Sessions");
  });


  it("opens the HTTPS settings section and enables the local route proxy", async () => {
    vi.mocked(getSettings).mockResolvedValue(settingsFixture);

    render(
      <QueryClientProvider client={createQueryClient()}>
        <I18nProvider initialLanguage="zh-CN">
          <SettingsScreen />
        </I18nProvider>
      </QueryClientProvider>,
    );

    await userEvent.click(await screen.findByRole("button", { name: /HTTPS/ }));
    expect(await screen.findByRole("heading", { name: "HTTPS" })).toBeInTheDocument();
    expect(screen.getByText("本地算力池 HTTPS")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("checkbox", { name: "为本地算力池启用 HTTPS" }));

    await waitFor(() => expect(enableRouteProxyHttps).toHaveBeenCalledTimes(1));
    // Both endpoints are listed: only the HTTP one goes into client configs, so
    // the user has to be able to tell which is which.
    expect(await screen.findByText("https://127.0.0.1:19528")).toBeInTheDocument();
    expect(screen.getByText("http://127.0.0.1:19527")).toBeInTheDocument();
  });

  it("shows untrusted guidance without blocking HTTPS controls", async () => {
    vi.mocked(getSettings).mockResolvedValue(settingsFixture);
    vi.mocked(getRouteProxyHttpsStatus).mockResolvedValue({
      ...httpsStatusFixture,
      enabled: true,
      certReady: true,
      manualInstructions: ["certutil.exe -user -addstore Root C:/tmp/root-ca.pem"],
    });

    render(
      <QueryClientProvider client={createQueryClient()}>
        <I18nProvider initialLanguage="zh-CN">
          <SettingsScreen />
        </I18nProvider>
      </QueryClientProvider>,
    );

    await userEvent.click(await screen.findByRole("button", { name: /HTTPS/ }));
    expect(await screen.findByText("根证书尚未受信任")).toBeInTheDocument();
    expect(screen.getByText("certutil.exe -user -addstore Root C:/tmp/root-ca.pem")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "重新导入根证书" })).toBeEnabled();
  });

  it("still surfaces recovery commands when trust could not be verified", async () => {
    // macOS lands here whenever the authorization prompt is dismissed: the
    // import reports success but the trust setting was never written. Gating the
    // panel on "untrusted" used to swallow the commands in exactly this state.
    vi.mocked(getSettings).mockResolvedValue(settingsFixture);
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

    render(
      <QueryClientProvider client={createQueryClient()}>
        <I18nProvider initialLanguage="zh-CN">
          <SettingsScreen />
        </I18nProvider>
      </QueryClientProvider>,
    );

    await userEvent.click(await screen.findByRole("button", { name: /HTTPS/ }));
    expect(
      await screen.findByText(
        "security add-trusted-cert -r trustRoot -k /Users/example/Library/Keychains/login.keychain-db /tmp/root-ca.pem",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Root CA installation completed, but the local trust store could not verify it"),
    ).toBeInTheDocument();
  });

});
