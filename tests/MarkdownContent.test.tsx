import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { MarkdownContent } from "../src/components/common/MarkdownContent";

describe("MarkdownContent", () => {
  it("renders headings, emphasis, inline code and lists", () => {
    render(
      <MarkdownContent
        content={"## 标题\n\n- **重点** 说明\n- `code` 片段\n\n1. 第一步\n2. 第二步"}
      />,
    );

    expect(screen.getByText("标题")).toBeInTheDocument();
    expect(screen.getByText("重点")).toHaveClass("font-semibold");
    expect(screen.getByText("code")).toBeInTheDocument();
    expect(screen.getAllByRole("list")).toHaveLength(2);
  });

  it("renders fenced code blocks without interpreting their contents", () => {
    render(
      <MarkdownContent
        content={"```js\nconst x = **not bold**;\n```"}
      />,
    );

    expect(screen.getByText(/const x = \*\*not bold\*\*/)).toBeInTheDocument();
    expect(screen.queryByText("not bold")).not.toBeInTheDocument();
  });

  it("only links to http(s) and keeps other link labels as text", () => {
    render(
      <MarkdownContent
        content={"[安全](https://example.com) [危险](javascript:alert(1))"}
      />,
    );

    expect(screen.getByRole("link", { name: "安全" })).toHaveAttribute(
      "href",
      "https://example.com",
    );
    expect(screen.queryByRole("link", { name: "危险" })).not.toBeInTheDocument();
    expect(document.body.textContent).toContain("危险");
  });
});
