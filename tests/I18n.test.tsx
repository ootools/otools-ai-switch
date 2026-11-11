import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { I18nProvider, normalizeLanguage, useI18n } from "../src/lib/i18n";

function LanguageView() {
  const { language, setLanguage, t } = useI18n();
  return (
    <>
      <output aria-label="当前语言">{language}</output>
      <span>{t("nav.settings")}</span>
      <button onClick={() => setLanguage("en")} type="button">English</button>
    </>
  );
}

beforeEach(() => {
  window.localStorage.clear();
});

describe("plugin language defaults", () => {
  it("opens in Chinese on first use and saves that preference", () => {
    render(<I18nProvider><LanguageView /></I18nProvider>);
    expect(screen.getByLabelText("当前语言")).toHaveTextContent("zh-CN");
    expect(screen.getByText("设置")).toBeInTheDocument();
    expect(document.documentElement.lang).toBe("zh-CN");
    expect(localStorage.getItem("ai-switch.language")).toBe("zh-CN");
  });

  it("respects a previously chosen English preference", () => {
    localStorage.setItem("ai-switch.language", "en");
    render(<I18nProvider><LanguageView /></I18nProvider>);
    expect(screen.getByLabelText("当前语言")).toHaveTextContent("en");
    expect(screen.getByText("Settings")).toBeInTheDocument();
  });

  it("falls back to Chinese for an unknown saved language", () => {
    localStorage.setItem("ai-switch.language", "invalid");
    render(<I18nProvider><LanguageView /></I18nProvider>);
    expect(screen.getByLabelText("当前语言")).toHaveTextContent("zh-CN");
    expect(normalizeLanguage(null)).toBe("zh-CN");
  });

  it("allows switching from the Chinese default to English", () => {
    render(<I18nProvider><LanguageView /></I18nProvider>);
    fireEvent.click(screen.getByRole("button", { name: "English" }));
    expect(screen.getByLabelText("当前语言")).toHaveTextContent("en");
    expect(screen.getByText("Settings")).toBeInTheDocument();
    expect(localStorage.getItem("ai-switch.language")).toBe("en");
  });

  it("uses Chinese translations for components outside the provider", () => {
    render(<LanguageView />);
    expect(screen.getByLabelText("当前语言")).toHaveTextContent("zh-CN");
    expect(screen.getByText("设置")).toBeInTheDocument();
  });
});
