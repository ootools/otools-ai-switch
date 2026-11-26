import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SubscriptionLoginDialog } from "../src/components/accounts/SubscriptionLoginDialog";
import { detectSubscriptionLogin, importSubscriptionLogin } from "../src/lib/api/client";
import { createQueryClient } from "../src/lib/query/queryClient";

vi.mock("../src/lib/api/client", () => ({
  detectSubscriptionLogin: vi.fn(),
  importSubscriptionLogin: vi.fn(),
}));

describe("SubscriptionLoginDialog", () => {
  beforeEach(() => {
    vi.mocked(detectSubscriptionLogin).mockReset();
    vi.mocked(detectSubscriptionLogin).mockResolvedValue("C:/Users/test/.claude/.credentials.json");
    vi.mocked(importSubscriptionLogin).mockReset();
    vi.mocked(importSubscriptionLogin).mockResolvedValue([
      { id: "acc-1", display_name: "Claude", email: null },
    ]);
  });

  it("detects the selected vendor's login file and imports it", async () => {
    const user = userEvent.setup();
    const onImported = vi.fn();
    render(
      <QueryClientProvider client={createQueryClient()}>
        <SubscriptionLoginDialog batchName="Team" onClose={vi.fn()} onImported={onImported} />
      </QueryClientProvider>,
    );

    expect(await screen.findByText("C:/Users/test/.claude/.credentials.json")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "导入登录" }));
    await waitFor(() =>
      expect(vi.mocked(importSubscriptionLogin)).toHaveBeenCalledWith(
        "claude",
        "Team",
        "C:/Users/test/.claude/.credentials.json",
      ),
    );
    await waitFor(() => expect(onImported).toHaveBeenCalled());
    expect(screen.getByText("已导入 1 个账号。")).toBeInTheDocument();
  });

  it("redetects when the vendor changes", async () => {
    const user = userEvent.setup();
    vi.mocked(detectSubscriptionLogin).mockResolvedValue(null);
    render(
      <QueryClientProvider client={createQueryClient()}>
        <SubscriptionLoginDialog batchName="Team" onClose={vi.fn()} onImported={vi.fn()} />
      </QueryClientProvider>,
    );

    await user.click(await screen.findByRole("button", { name: "Gemini" }));
    await waitFor(() =>
      expect(vi.mocked(detectSubscriptionLogin)).toHaveBeenLastCalledWith("gemini"),
    );
  });
});
