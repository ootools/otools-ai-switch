import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { motion } from "motion/react";
import {
  Bell,
  Bot,
  Database,
  Globe,
  Network,
  ScrollText,
  Settings2,
  ShieldCheck,
} from "lucide-react";
import type { ComponentType } from "react";
import { useState } from "react";
import {
  getRoutePool,
  getSettings,
  saveSettings,
  setRoutePoolBaselineModels,
} from "../lib/api/client";
import {
  accountModalities,
  createDefaultModalityVisibility,
  modalityLabelKeys,
  type AccountModality,
  type ModalityVisibility,
} from "../lib/accountModalities";
import { normalizeLanguage, supportedLanguages, useI18n, type Language, type TranslationKey } from "../lib/i18n";
import { normalizeThemePreference, type ThemePreference } from "../lib/theme";
import { MotionPresence } from "../components/motion/MotionPrimitives";
import { CodexBaselineModelsDialog } from "../components/accounts/CodexBaselineModelsDialog";
import { NotificationSettings } from "../components/settings/notification-settings";
import { PiiModelSettings } from "../components/settings/PiiModelSettings";
import { RouteProxyHttpsSettings } from "../components/settings/route-proxy-https-settings";
import { UsageStorageSettings } from "../components/settings/usage-storage-settings";
import { OperationLogScreen } from "./OperationLogScreen";

type SettingsTab =
  | "general"
  | "agents"
  | "privacy"
  | "network"
  | "notifications"
  | "proxy"
  | "data"
  | "logs";

const tabs: Array<{ key: SettingsTab; labelKey: TranslationKey; icon: ComponentType<{ className?: string }> }> = [
  { key: "general", labelKey: "settings.tabs.general", icon: Settings2 },
  { key: "agents", labelKey: "settings.tabs.agents", icon: Bot },
  { key: "privacy", labelKey: "settings.tabs.privacy", icon: ShieldCheck },
  { key: "network", labelKey: "settings.tabs.network", icon: Globe },
  { key: "notifications", labelKey: "settings.tabs.notifications", icon: Bell },
  { key: "proxy", labelKey: "settings.tabs.proxy", icon: Network },
  { key: "data", labelKey: "settings.tabs.data", icon: Database },
  { key: "logs", labelKey: "settings.tabs.logs", icon: ScrollText },
];

const themeOptions: {
  value: ThemePreference;
  labelKey: "settings.theme.system" | "settings.theme.light" | "settings.theme.dark";
}[] = [
  { value: "system", labelKey: "settings.theme.system" },
  { value: "light", labelKey: "settings.theme.light" },
  { value: "dark", labelKey: "settings.theme.dark" },
];

type SettingsScreenProps = {
  onOpenFeature?: (screen: string) => void;
  modalityVisibility?: ModalityVisibility;
  onModalityVisibilityChange?: (modality: AccountModality, visible: boolean) => void;
};

export function SettingsScreen({
  onOpenFeature,
  modalityVisibility,
  onModalityVisibilityChange,
}: SettingsScreenProps) {
  const queryClient = useQueryClient();
  const { language, setLanguage, t } = useI18n();
  const [activeTab, setActiveTab] = useState<SettingsTab>("general");
  const [localModalityVisibility, setLocalModalityVisibility] = useState(createDefaultModalityVisibility);
  // Drafts overlay loaded settings so edits feel immediate and reset cleanly
  // when settings reload underneath. Hooks stay above the loading early-returns.
  const [proxyEnabledDraft, setProxyEnabledDraft] = useState<boolean | null>(null);
  const [proxyUrlDraft, setProxyUrlDraft] = useState<string | null>(null);
  const [proxyError, setProxyError] = useState<string | null>(null);
  const [requestBodyLimitDraft, setRequestBodyLimitDraft] = useState<string | null>(null);
  const [requestBodyLimitError, setRequestBodyLimitError] = useState(false);
  const [compactWindowDraft, setCompactWindowDraft] = useState<string | null>(null);
  const [privacyRulesDraft, setPrivacyRulesDraft] = useState<string | null>(null);
  const [privacyRuleError, setPrivacyRuleError] = useState(false);
  const [baselineModelsDialogOpen, setBaselineModelsDialogOpen] = useState(false);
  const settingsQuery = useQuery({ queryKey: ["settings"], queryFn: getSettings });
  const routePoolQuery = useQuery({
    queryKey: ["route-pool", "codex", "active"],
    queryFn: () => getRoutePool("codex"),
  });
  const saveBaselineModelsMutation = useMutation({
    mutationFn: (models: string[]) => setRoutePoolBaselineModels({ platform: "codex", models }),
    onSuccess: (state) => {
      queryClient.setQueryData(["route-pool", "codex", "active"], state);
      void queryClient.invalidateQueries({ queryKey: ["route-pool", "codex"] });
      void queryClient.invalidateQueries({ queryKey: ["route-credential-page", "codex"] });
      void queryClient.invalidateQueries({ queryKey: ["route-config-stale"] });
      setBaselineModelsDialogOpen(false);
    },
  });
  const saveMutation = useMutation({
    mutationFn: saveSettings,
    onSuccess: (settings) => {
      queryClient.setQueryData(["settings"], settings);
      setLanguage(normalizeLanguage(settings.language));
    },
  });

  if (settingsQuery.isLoading) {
    return <p className="text-sm text-stone-500">{t("settings.loading")}</p>;
  }
  if (!settingsQuery.data) {
    return <p className="text-sm text-red-700">{t("settings.error")}</p>;
  }

  const settings = settingsQuery.data;
  const effectiveModalityVisibility = modalityVisibility ?? localModalityVisibility;
  const handleModalityVisibilityChange = (modality: AccountModality, visible: boolean) => {
    if (onModalityVisibilityChange) {
      onModalityVisibilityChange(modality, visible);
      return;
    }
    setLocalModalityVisibility((current) => ({ ...current, [modality]: visible }));
  };
  const handleLanguageChange = (nextLanguage: Language) => {
    setLanguage(nextLanguage);
    saveMutation.mutate({ ...settings, language: nextLanguage });
  };

  const PROXY_EXAMPLE = "http://127.0.0.1:7890";
  const proxyEnabled = proxyEnabledDraft ?? settings.proxy_enabled;
  const proxyUrl = proxyUrlDraft ?? settings.proxy_url ?? "";
  const saveProxy = (nextEnabled: boolean, nextUrl: string) => {
    if (nextEnabled && !nextUrl.trim()) {
      setProxyError(t("settings.proxy.required"));
      return;
    }
    setProxyError(null);
    saveMutation.mutate({
      ...settings,
      proxy_enabled: nextEnabled,
      proxy_url: nextUrl.trim() || null,
    });
  };
  const handleProxyEnabledChange = (next: boolean) => {
    setProxyEnabledDraft(next);
    if (next && !proxyUrl.trim()) {
      setProxyError(t("settings.proxy.required"));
      return;
    }
    setProxyError(null);
    saveProxy(next, proxyUrl);
  };

  const requestBodyLimit = settings.route_proxy_request_body_limit_mib ?? 128;
  const saveRequestBodyLimit = () => {
    const draft = requestBodyLimitDraft ?? String(requestBodyLimit);
    const value = Number(draft);
    if (!draft.trim() || !Number.isInteger(value) || value < 0 || value > 0xffff_ffff) {
      setRequestBodyLimitError(true);
      return;
    }
    setRequestBodyLimitError(false);
    if (value !== requestBodyLimit) {
      saveMutation.mutate(
        { ...settings, route_proxy_request_body_limit_mib: value },
        { onSuccess: () => setRequestBodyLimitDraft(null) },
      );
    }
  };

  const cardClass = "space-y-3 rounded-2xl border border-stone-200 bg-white/82 p-4 shadow-sm";
  const inputClass = "rounded-xl border border-stone-200 bg-white px-3 py-2 text-[13px] font-medium text-stone-900 shadow-sm outline-none motion-control focus:border-blue-400 focus:ring-2 focus:ring-blue-100";

  return (
    <section className="space-y-3">
      <div className="rounded-2xl border border-stone-200 bg-white/82 px-4 py-3 shadow-sm">
        <p className="text-[11px] font-semibold uppercase tracking-wide text-stone-400">
          {t("settings.hub.kicker")}
        </p>
        <h1 className="mt-0.5 text-lg font-semibold tracking-tight text-stone-950">
          {t("settings.title")}
        </h1>
      </div>

      <div
        aria-label={t("settings.title")}
        className="flex flex-wrap gap-1 rounded-2xl border border-stone-200 bg-white/82 p-1.5 shadow-sm"
        role="tablist"
      >
        {tabs.map((tab) => {
          const Icon = tab.icon;
          const active = activeTab === tab.key;
          return (
            <button
              aria-selected={active}
              className={`inline-flex items-center gap-1.5 rounded-xl px-3 py-1.5 text-[12px] font-semibold motion-control ${
                active
                  ? "bg-stone-900 text-white shadow-sm"
                  : "text-stone-600 hover:bg-stone-100 hover:text-stone-950"
              }`}
              key={tab.key}
              onClick={() => setActiveTab(tab.key)}
              role="tab"
              type="button"
            >
              <Icon aria-hidden="true" className="h-3.5 w-3.5" />
              {t(tab.labelKey)}
            </button>
          );
        })}
      </div>

      <MotionPresence>
        <motion.div
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -8 }}
          initial={{ opacity: 0, y: 8 }}
          key={activeTab}
          transition={{ duration: 0.24, ease: [0.22, 1, 0.36, 1] }}
        >
          {activeTab === "general" && (
            <div className={cardClass}>
              <h2 className="text-[15px] font-semibold text-stone-950">{t("settings.app.title")}</h2>
              <label className="flex max-w-sm flex-col gap-1.5 text-[12px] font-semibold text-stone-600">
                <span>{t("settings.language")}</span>
                <select
                  aria-label={t("settings.language")}
                  className={inputClass}
                  disabled={saveMutation.isPending}
                  onChange={(event) => handleLanguageChange(event.target.value as Language)}
                  value={language}
                >
                  {supportedLanguages.map((option) => (
                    <option key={option.code} value={option.code}>{option.label}</option>
                  ))}
                </select>
              </label>
              <label className="flex max-w-sm flex-col gap-1.5 text-[12px] font-semibold text-stone-600">
                <span>{t("settings.theme")}</span>
                <select
                  aria-label={t("settings.theme")}
                  className={inputClass}
                  disabled={saveMutation.isPending}
                  onChange={(event) => saveMutation.mutate({ ...settings, theme: event.target.value })}
                  value={normalizeThemePreference(settings.theme)}
                >
                  {themeOptions.map((option) => (
                    <option key={option.value} value={option.value}>{t(option.labelKey)}</option>
                  ))}
                </select>
              </label>
              {saveMutation.data && <p className="text-[13px] font-medium text-emerald-700">{t("settings.saved")}</p>}
              {saveMutation.error && <p className="text-[13px] font-medium text-red-700">{t("settings.saveError")}</p>}
            </div>
          )}

          {activeTab === "agents" && (
            <div className="space-y-3">
              <div className={cardClass}>
                <h2 className="text-[15px] font-semibold text-stone-950">{t("settings.modalityVisibility.title")}</h2>
                <p className="mt-1 text-[11px] font-medium text-stone-500">{t("settings.modalityVisibility.subtitle")}</p>
                <div className="grid gap-2 sm:grid-cols-2">
                  {accountModalities.map((modality) => (
                    <label
                      className="flex items-center gap-2 rounded-lg border border-stone-200 bg-white px-3 py-2 text-[12px] font-semibold text-stone-700"
                      key={modality}
                    >
                      <input
                        aria-label={t("settings.modalityVisibility.show", { modality: t(modalityLabelKeys[modality]) })}
                        checked={effectiveModalityVisibility[modality]}
                        className="accent-blue-600"
                        onChange={(event) => handleModalityVisibilityChange(modality, event.target.checked)}
                        type="checkbox"
                      />
                      <span>{t("settings.modalityVisibility.show", { modality: t(modalityLabelKeys[modality]) })}</span>
                    </label>
                  ))}
                </div>
              </div>
              <div className={cardClass}>
                <h2 className="text-[15px] font-semibold text-stone-950">Codex 基线模型</h2>
                <p className="text-[12px] text-stone-500">
                  未配置模型映射的账号会贡献这些基线模型；保存后算力池模型清单立即使用新顺序。
                </p>
                <button
                  aria-label="配置 Codex 基线模型"
                  className="w-fit rounded-xl border border-stone-300 bg-white px-3 py-2 text-[12px] font-semibold text-stone-700 motion-control hover:bg-stone-50"
                  onClick={() => {
                    saveBaselineModelsMutation.reset();
                    setBaselineModelsDialogOpen(true);
                  }}
                  type="button"
                >
                  配置 Codex 基线模型
                </button>
              </div>
              <div className={cardClass}>
                <h2 className="text-[15px] font-semibold text-stone-950">{t("settings.longConversation.title")}</h2>
                <p className="text-[12px] text-stone-500">{t("settings.longConversation.hint")}</p>
                <label className="flex max-w-xl items-start gap-2 rounded-xl border border-stone-200 bg-white px-3 py-2.5 text-[12px] font-semibold text-stone-700">
                  <input
                    aria-label={t("settings.longConversation.enable")}
                    checked={settings.long_conversation_enabled ?? false}
                    className="mt-0.5"
                    disabled={saveMutation.isPending}
                    onChange={(event) => saveMutation.mutate({ ...settings, long_conversation_enabled: event.target.checked })}
                    type="checkbox"
                  />
                  <span>{t("settings.longConversation.enable")}</span>
                </label>
                <label className="flex max-w-sm flex-col gap-1.5 text-[12px] font-semibold text-stone-600">
                  <span>{t("settings.longConversation.window")}</span>
                  <input
                    aria-label={t("settings.longConversation.window")}
                    className={inputClass}
                    disabled={saveMutation.isPending}
                    min={1}
                    step={1000}
                    type="number"
                    value={compactWindowDraft ?? settings.long_conversation_compact_window ?? 272000}
                    onChange={(event) => setCompactWindowDraft(event.target.value)}
                    onBlur={() => {
                      const value = Number(compactWindowDraft);
                      if (Number.isInteger(value) && value > 0) {
                        saveMutation.mutate({ ...settings, long_conversation_compact_window: value });
                      }
                      setCompactWindowDraft(null);
                    }}
                  />
                </label>
              </div>
            </div>
          )}

          {activeTab === "privacy" && (
            <div className={cardClass}>
              <div className="flex items-center justify-between gap-3">
                <h2 className="text-[15px] font-semibold text-stone-950">{t("settings.privacy.title")}</h2>
                <button
                  className="rounded-lg border border-stone-300 bg-white px-2.5 py-1.5 text-[11px] font-semibold text-stone-700 motion-control hover:bg-stone-50"
                  onClick={() => onOpenFeature?.("Insights")}
                  type="button"
                >
                  {t("nav.insights")}
                </button>
              </div>
              <label className="flex items-center gap-2 text-[13px] font-medium text-stone-700">
                <input
                  aria-label={t("settings.privacy.title")}
                  checked={settings.privacy_masking_enabled ?? false}
                  className="accent-blue-600"
                  onChange={(event) => saveMutation.mutate({ ...settings, privacy_masking_enabled: event.target.checked })}
                  type="checkbox"
                />
                开启本地脱敏
              </label>
              <label className="flex flex-col gap-1.5 text-[12px] font-semibold text-stone-600">
                规则（JSON）
                <textarea
                  aria-label="隐私脱敏规则 JSON"
                  className="min-h-24 rounded-xl border border-stone-200 bg-stone-50 px-3 py-2 font-mono text-[12px] outline-none focus:border-blue-400"
                  onChange={(event) => {
                    setPrivacyRulesDraft(event.target.value);
                    setPrivacyRuleError(false);
                  }}
                  value={
                    privacyRulesDraft ??
                    settings.privacy_mask_rules_json ??
                    '{"mask_secrets":true,"mask_emails":true,"custom_words":[]}'
                  }
                />
              </label>
              {privacyRuleError ? (
                <p className="text-[12px] text-red-700" role="alert">规则不是合法 JSON。</p>
              ) : null}
              <button
                className="rounded-xl border border-stone-300 bg-white px-3 py-2 text-[12px] font-semibold text-stone-700 motion-control hover:bg-stone-50"
                onClick={() => {
                  const text =
                    privacyRulesDraft ??
                    settings.privacy_mask_rules_json ??
                    '{"mask_secrets":true,"mask_emails":true,"custom_words":[]}';
                  try {
                    JSON.parse(text);
                    setPrivacyRuleError(false);
                    saveMutation.mutate({ ...settings, privacy_mask_rules_json: text });
                  } catch {
                    setPrivacyRuleError(true);
                  }
                }}
                type="button"
              >
                保存规则
              </button>
              <p className="text-[11px] leading-5 text-stone-500">
                v1 规则识别密钥 / 令牌、邮箱和自定义词；可选本地模型进一步识别姓名与地址（见下方）。
              </p>
              <PiiModelSettings settings={settings} />
            </div>
          )}

          {activeTab === "network" && <RouteProxyHttpsSettings />}

          {activeTab === "notifications" && <NotificationSettings settings={settings} />}

          {activeTab === "proxy" && (
            <div className={cardClass}>
              <h2 className="text-[15px] font-semibold text-stone-950">{t("settings.proxy.title")}</h2>
              <p className="text-[12px] text-stone-500">{t("settings.proxy.subtitle")}</p>
              <label className="flex max-w-xl items-start gap-2 rounded-xl border border-stone-200 bg-white px-3 py-2.5 text-[12px] font-semibold text-stone-700">
                <input
                  aria-label={t("settings.proxy.enable")}
                  checked={proxyEnabled}
                  className="mt-0.5"
                  disabled={saveMutation.isPending}
                  onChange={(event) => handleProxyEnabledChange(event.target.checked)}
                  type="checkbox"
                />
                <span>{t("settings.proxy.enable")}</span>
              </label>
              <label className="flex max-w-sm flex-col gap-1.5 text-[12px] font-semibold text-stone-600">
                <span>{t("settings.proxy.address")}</span>
                <input
                  aria-label={t("settings.proxy.address")}
                  className={inputClass}
                  disabled={saveMutation.isPending}
                  onBlur={() => {
                    if (proxyEnabled && !proxyUrl.trim()) {
                      setProxyError(t("settings.proxy.required"));
                      return;
                    }
                    setProxyError(null);
                    if (proxyUrl.trim() !== (settings.proxy_url ?? "")) saveProxy(proxyEnabled, proxyUrl);
                  }}
                  onChange={(event) => {
                    setProxyUrlDraft(event.target.value);
                    if (event.target.value.trim()) setProxyError(null);
                  }}
                  placeholder={PROXY_EXAMPLE}
                  type="text"
                  value={proxyUrl}
                />
                {proxyError ? (
                  <span className="text-[11px] font-medium text-red-700">{proxyError}</span>
                ) : (
                  <span className="text-[11px] font-medium text-stone-500">{t("settings.proxy.hint")}</span>
                )}
              </label>
            </div>
          )}

          {activeTab === "data" && (
            <div className="space-y-3">
              <div className={cardClass}>
                <h2 className="text-[15px] font-semibold text-stone-950">{t("settings.app.title")}</h2>
                <p className="rounded-xl border border-stone-200 bg-stone-50 px-3 py-2 text-[12px] text-stone-600">
                  {t("settings.dataDir", { path: settings.data_dir })}
                </p>
                <UsageStorageSettings />
              </div>
              <div className={cardClass}>
                <h2 className="text-[15px] font-semibold text-stone-950">{t("settings.requestBodyLimit.title")}</h2>
                <label className="flex max-w-sm flex-col gap-1.5 text-[12px] font-semibold text-stone-600">
                  <span>{t("settings.requestBodyLimit.label")}</span>
                  <input
                    aria-label={t("settings.requestBodyLimit.label")}
                    aria-invalid={requestBodyLimitError}
                    aria-describedby="request-body-limit-hint"
                    className={inputClass}
                    disabled={saveMutation.isPending}
                    min={0}
                    step={1}
                    type="number"
                    value={requestBodyLimitDraft ?? requestBodyLimit}
                    onChange={(event) => {
                      setRequestBodyLimitDraft(event.target.value);
                      setRequestBodyLimitError(false);
                    }}
                    onBlur={saveRequestBodyLimit}
                  />
                </label>
                <p id="request-body-limit-hint" className="text-[12px] text-stone-500">{t("settings.requestBodyLimit.hint")}</p>
                {requestBodyLimitError && (
                  <p role="alert" className="text-[12px] font-medium text-red-700">{t("settings.requestBodyLimit.invalid")}</p>
                )}
              </div>
            </div>
          )}

          {activeTab === "logs" && <OperationLogScreen />}
        </motion.div>
      </MotionPresence>

      {baselineModelsDialogOpen && (
        <CodexBaselineModelsDialog
          defaultModels={routePoolQuery.data?.default_baseline_models ?? []}
          error={
            saveBaselineModelsMutation.error
              ? "保存 Codex 基线模型失败。"
              : null
          }
          loading={saveBaselineModelsMutation.isPending}
          models={routePoolQuery.data?.baseline_models ?? []}
          onClose={() => setBaselineModelsDialogOpen(false)}
          onSave={(models) => saveBaselineModelsMutation.mutate(models)}
        />
      )}
    </section>
  );
}
