import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AgentLogo } from "../src/components/brand/AgentLogo";
import { agentClientInfo } from "../src/lib/agentClients";
import { ClientIcon } from "../src/lib/clientSetup";

describe("AgentLogo", () => {
  it.each(["codex", "claude_code", "gemini_cli", "grok", "opencode", "openclaw", "hermes", "zcode", "workbuddy", "workbuddy_ai", "codebuddy_cli", "qoder_cli", "qoder_cn_cli", "deepseek_harness", "deepseek_harness_desktop"])("renders %s locally with an accessible name", (clientKey) => {
    const { container } = render(<AgentLogo clientKey={clientKey} />);
    expect(screen.getByRole("img", { name: agentClientInfo(clientKey).label })).toBeInTheDocument();
    expect(container.querySelector("svg, img")).toBeInTheDocument();
    for (const image of container.querySelectorAll("img")) {
      expect(image.getAttribute("src")).not.toMatch(/^https?:/);
    }
  });

  it("uses the official local Harness artwork in client setup as well as the traffic view", () => {
    const { container } = render(<ClientIcon clientKey="deepseek_harness" />);
    expect(container.querySelector("img")?.getAttribute("src")).toContain("brand/deepseek-harness.svg");
  });

  it("uses a neutral mark for unknown clients, including object prototype names", () => {
    render(<AgentLogo clientKey="__proto__" />);
    expect(screen.getByRole("img", { name: "__proto__" })).toBeInTheDocument();
    expect(agentClientInfo("__proto__").color).toBe("#64748b");
  });
});
