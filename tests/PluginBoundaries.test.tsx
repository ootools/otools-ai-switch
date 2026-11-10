import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
import { AppLayout } from "../src/components/layout/AppLayout";
import { I18nProvider } from "../src/lib/i18n";
import { createQueryClient } from "../src/lib/query/queryClient";
import { SessionsScreen } from "../src/screens/SessionsScreen";
import { getSessionMessages, listSessions } from "../src/lib/api/client";

vi.mock("../src/lib/api/client", () => ({ listSessions: vi.fn(), getSessionMessages: vi.fn(), openSessionTerminal: vi.fn() }));
vi.mock("../src/lib/transport", () => ({ isDesktop: () => true }));
beforeEach(() => window.localStorage.clear());

it("opens image generation without a plugin toggle and omits excluded navigation", async () => {
  const navigate = vi.fn();
  render(<I18nProvider initialLanguage="en"><AppLayout activeScreen="PoolGeneral" onNavigate={navigate} onToggleSidebar={() => {}} sidebarCollapsed={false}><p>Accounts</p></AppLayout></I18nProvider>);
  await userEvent.click(screen.getByRole("button", { name: "Image Studio" }));
  expect(navigate).toHaveBeenCalledWith("ImageGen");
  expect(screen.queryByRole("button", { name: /Vibe|SaaS|Updates/i })).not.toBeInTheDocument();
});

it("reads a session and exposes no terminal launch action", async () => {
  vi.mocked(listSessions).mockResolvedValue([{ providerId: "codex", sessionId: "one", title: "Read me", projectDir: "D:/demo", createdAt: null, lastActiveAt: null, sourcePath: "D:/demo/session.jsonl", resumeCommand: "codex resume one" }]);
  vi.mocked(getSessionMessages).mockResolvedValue([{ role: "assistant", content: "Saved conversation", ts: null }]);
  render(<I18nProvider initialLanguage="en"><QueryClientProvider client={createQueryClient()}><SessionsScreen /></QueryClientProvider></I18nProvider>);
  expect(await screen.findByText("Saved conversation")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /terminal/i })).not.toBeInTheDocument();
});
