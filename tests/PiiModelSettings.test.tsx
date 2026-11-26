import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { PiiModelSettings } from "../src/components/settings/PiiModelSettings";
import {
  cancelPiiModelDownload,
  deletePiiModel,
  getPiiModelStatus,
  saveSettings,
  startPiiModelDownload,
  warmPiiModel,
} from "../src/lib/api/client";
import { createQueryClient } from "../src/lib/query/queryClient";
import type { AppSettings } from "../src/lib/api/types";

vi.mock("../src/lib/api/client", () => ({
  getPiiModelStatus: vi.fn(),
  startPiiModelDownload: vi.fn(),
  cancelPiiModelDownload: vi.fn(),
  deletePiiModel: vi.fn(),
  warmPiiModel: vi.fn(),
  saveSettings: vi.fn(),
}));

const subscribe = vi.fn(() => Promise.resolve(vi.fn()));
vi.mock("../src/lib/transport", () => ({
  getTransport: vi.fn(() => ({ subscribe })),
}));

const settings: AppSettings = {
  language: "zh-CN",
  theme: "system",
  copy_import_sources: false,
  logging_enabled: true,
  secret_storage: "keyring",
  data_dir: "/tmp/ai-switch",
  ccswitch_deeplink_compat_enabled: false,
  close_to_tray: true,
  image_generation_enabled: false,
  proxy_enabled: false,
  route_proxy_request_body_limit_mib: 128,
  privacy_masking_enabled: true,
  pii_model_enabled: false,
};

describe("PiiModelSettings", () => {
  beforeEach(() => {
    vi.mocked(getPiiModelStatus).mockReset();
    vi.mocked(getPiiModelStatus).mockResolvedValue({
      supported: true,
      file_state: "missing",
      runtime_state: "stopped",
      downloaded_bytes: 0,
      total_bytes: 40_000_000,
      device: "",
      load_ms: 0,
      model_size: 39_000_000,
      ort_size: 7_000_000,
      last_error: null,
    });
    vi.mocked(startPiiModelDownload).mockReset();
    vi.mocked(startPiiModelDownload).mockResolvedValue(undefined);
    vi.mocked(cancelPiiModelDownload).mockReset();
    vi.mocked(cancelPiiModelDownload).mockResolvedValue(undefined);
    vi.mocked(deletePiiModel).mockReset();
    vi.mocked(deletePiiModel).mockResolvedValue(undefined);
    vi.mocked(warmPiiModel).mockReset();
    vi.mocked(warmPiiModel).mockResolvedValue({
      supported: true,
      file_state: "ready",
      runtime_state: "ready",
      downloaded_bytes: 40_000_000,
      total_bytes: 40_000_000,
      device: "cpu",
      load_ms: 123,
      model_size: 39_000_000,
      ort_size: 7_000_000,
      last_error: null,
    });
    vi.mocked(saveSettings).mockReset();
    vi.mocked(saveSettings).mockImplementation(async (value) => value);
    subscribe.mockClear();
  });

  it("shows status and starts a download with the configured mirror", async () => {
    const user = userEvent.setup();
    render(
      <QueryClientProvider client={createQueryClient()}>
        <PiiModelSettings settings={settings} />
      </QueryClientProvider>,
    );

    expect(await screen.findByText(/当前平台可下载/)).toBeInTheDocument();
    expect(screen.getByText(/文件状态：\s*missing/)).toBeInTheDocument();
    expect(subscribe).toHaveBeenCalledWith("pii-model-download-progress", expect.any(Function));

    await user.type(screen.getByLabelText("下载镜像地址"), "https://mirror.example/pii");
    await user.click(screen.getByRole("button", { name: "保存镜像与阈值" }));
    await waitFor(() =>
      expect(vi.mocked(saveSettings).mock.calls[0]?.[0]).toEqual(
        expect.objectContaining({ pii_model_mirror_base_url: "https://mirror.example/pii" }),
      ),
    );

    await user.click(screen.getByRole("button", { name: "下载模型与运行时" }));
    await waitFor(() =>
      expect(vi.mocked(startPiiModelDownload)).toHaveBeenCalledWith("https://mirror.example/pii"),
    );
    expect(screen.getByText("已开始下载，进度见下方。")).toBeInTheDocument();
  });

  it("toggles the model through settings", async () => {
    const user = userEvent.setup();
    render(
      <QueryClientProvider client={createQueryClient()}>
        <PiiModelSettings settings={settings} />
      </QueryClientProvider>,
    );

    await user.click(await screen.findByLabelText("启用本地 PII 模型"));
    await waitFor(() =>
      expect(vi.mocked(saveSettings).mock.calls[0]?.[0]).toEqual(
        expect.objectContaining({ pii_model_enabled: true }),
      ),
    );
  });
});
