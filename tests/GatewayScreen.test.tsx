import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  getRouteProxyStatus,
  getRouteProxyKey,
  routePoolTestModel,
  startRouteProxy,
  stopRouteProxy,
  subscribeRouteProxyLiveLog,
  unsubscribeRouteProxyLiveLog,
} from "../src/lib/api/client";
import { I18nProvider } from "../src/lib/i18n";
import { createQueryClient } from "../src/lib/query/queryClient";
import { fetchRouteProxyModels } from "../src/lib/routeProxyModels";
import { GatewayScreen } from "../src/screens/GatewayScreen";

vi.mock("../src/lib/api/client", () => ({
  getRouteProxyStatus: vi.fn(),
  getRouteProxyKey: vi.fn(),
  startRouteProxy: vi.fn(),
  stopRouteProxy: vi.fn(),
  routePoolTestModel: vi.fn(),
  subscribeRouteProxyLiveLog: vi.fn(),
  unsubscribeRouteProxyLiveLog: vi.fn(),
}));

vi.mock("../src/lib/routeProxyModels", () => ({
  fetchRouteProxyModels: vi.fn(),
}));

const runningStatus = {
  running: true,
  route_access_enabled: true,
  bind_host: "127.0.0.1",
  port: 43111,
  base_url: "http://127.0.0.1:43111/v1",
  https_base_url: "https://127.0.0.1:43111/v1/",
};

function renderGateway() {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <I18nProvider initialLanguage="zh-CN">
        <GatewayScreen />
      </I18nProvider>
    </QueryClientProvider>,
  );
}

describe("GatewayScreen", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(getRouteProxyStatus).mockResolvedValue(runningStatus);
    vi.mocked(getRouteProxyKey).mockResolvedValue("sk-ai-switch-test");
    vi.mocked(fetchRouteProxyModels).mockResolvedValue([{ id: "gpt-5" }, { id: "gpt-5-mini" }]);
    vi.mocked(subscribeRouteProxyLiveLog).mockResolvedValue([]);
    vi.mocked(unsubscribeRouteProxyLiveLog).mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: vi.fn().mockResolvedValue(undefined) },
    });
  });

  it("shows gateway status, endpoints, model list and live log section", async () => {
    renderGateway();
    expect(await screen.findByText("http://127.0.0.1:43111/v1")).toBeInTheDocument();
    expect(screen.getByText("https://127.0.0.1:43111/v1/")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "停止 AI 服务" })).toBeInTheDocument();
    expect(await screen.findByText("gpt-5")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "实时日志" })).toBeInTheDocument();
    expect(screen.getByText("暂无请求记录。")).toBeInTheDocument();
  });

  it("copies the HTTP endpoint", async () => {
    renderGateway();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    await userEvent.click(await screen.findByRole("button", { name: "复制 Base URL" }));
    expect(writeText).toHaveBeenCalledWith("http://127.0.0.1:43111/v1");
    expect(screen.getByRole("button", { name: "复制 Base URL" })).toBeInTheDocument();
  });

  it("starts and stops the gateway through the API", async () => {
    vi.mocked(getRouteProxyStatus)
      .mockResolvedValueOnce(runningStatus)
      .mockResolvedValue({ ...runningStatus, running: false });
    vi.mocked(stopRouteProxy).mockResolvedValue({ ...runningStatus, running: false });
    vi.mocked(startRouteProxy).mockResolvedValue(runningStatus);
    renderGateway();
    await userEvent.click(await screen.findByRole("button", { name: "停止 AI 服务" }));
    await waitFor(() => expect(stopRouteProxy).toHaveBeenCalledTimes(1));
    expect(await screen.findByRole("button", { name: "启动 AI 服务" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "启动 AI 服务" }));
    await waitFor(() => expect(startRouteProxy).toHaveBeenCalledTimes(1));
  });

  it("runs a model test with the chosen model and tool-call flag", async () => {
    vi.mocked(routePoolTestModel).mockResolvedValue({
      platform: "codex",
      selected_account_id: "a1",
      selected_account_name: "acct-1",
      interface_format: "openai",
      request_path: "/v1/chat/completions",
      request_body_json: "{}",
      response_body: "{}",
      success: true,
      duration_ms: 12,
      stats: {
        member_count: 1,
        request_count: 0,
        token_count: 0,
        input_token_count: 0,
        output_token_count: 0,
        cache_token_count: 0,
        cost_micros: 0,
        recent_logs: [],
        requests: [],
        request_row_count: 0,
        request_page: 1,
        request_page_size: 20,
      },
    });
    renderGateway();
    await userEvent.type(await screen.findByLabelText("网关测试模型"), "gpt-5");
    await userEvent.click(screen.getByLabelText("测试工具调用能力"));
    await userEvent.click(screen.getByRole("button", { name: "模型测试" }));
    await waitFor(() =>
      expect(routePoolTestModel).toHaveBeenCalledWith({
        platform: "codex",
        model: "gpt-5",
        test_tool_call: true,
      }),
    );
    expect(await screen.findByText("acct-1 · openai")).toBeInTheDocument();
  });

  it("disables model testing while the gateway is stopped", async () => {
    vi.mocked(getRouteProxyStatus).mockResolvedValue({ ...runningStatus, running: false });
    renderGateway();
    expect(await screen.findByRole("button", { name: "模型测试" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "启动 AI 服务" })).toBeInTheDocument();
  });

  it("shows live log entries returned by the host", async () => {
    vi.mocked(subscribeRouteProxyLiveLog).mockResolvedValue([
      {
        id: "log-1",
        platform: "codex",
        credential_id: "a1",
        credential_name: "acct-1",
        attempt: 1,
        path: "/v1/chat/completions",
        requested_model: "gpt-5-live",
        success: true,
        truncated: false,
        created_at: "2026-11-27T00:00:00Z",
        duration_ms: 42,
      },
    ]);
    renderGateway();
    expect(await screen.findByText("gpt-5-live")).toBeInTheDocument();
    expect(screen.getByText("42 ms")).toBeInTheDocument();
  });
});
