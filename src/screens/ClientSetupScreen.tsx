import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { HelpCircle, Plug } from "lucide-react";
import { useEffect, useMemo, useState, type FormEvent, type ReactNode } from "react";
import { EndpointRow } from "../components/accounts/EndpointRow";
import { ClaudeSlotSettingsDialog } from "../components/accounts/ClaudeSlotSettingsDialog";
import {
  getClaudeSlotPlan,
  getCodexLoginState,
  getClientDirectModes,
  getRoutePool,
  getRouteProxyKey,
  getRouteProxyStatus,
  getSettings,
  listConfigWriteClients,
  listRouteCredentials,
  restoreNativeConfig,
  rollbackConfigSnapshot,
  routeConfigWriteIsStale,
  saveSettings,
  setRoutePoolModelMode,
  updateClaudeAccountSlotMappings,
  writeRouteProxyConfigs,
} from "../lib/api/client";
import type {
  ConfigWriteClientStatus,
  ConfigWriteOutcome,
  RouteCredential,
  RoutePoolModelMode,
} from "../lib/api/types";
import {
  CLAUDE_CODE_CLIENT_KEY,
  ClientIcon,
  DEFAULT_AGENT_PLATFORM,
  MESSAGES_AGENT_PLATFORM,
  defaultAgentInterfaceFormat,
  interfaceFormatHints,
  interfaceFormatLabels,
  platformForInterfaceFormat,
  type AgentInterfaceFormat,
} from "../lib/clientSetup";
import { poolImageModels as poolImageModelsOf } from "../lib/imageCapability";
import {
  capabilityReason,
  findPlatformCapability,
  operationEnabled,
} from "../lib/platformCapabilities";
import { usePlatformCapabilities } from "../lib/query/platformCapabilities";
import { imageGenerationEndpoint, routeProxyEndpointForPlatform } from "../lib/routeProxyEndpoint";
import { isDesktop } from "../lib/transport";

const fileStatusLabels: Record<string, string> = {
  missing: "未建立",
  managed: "已接管",
  unmanaged: "未接管",
  invalid: "无法解析",
  error: "无法读取",
};

const fileStatusTones: Record<string, string> = {
  managed: "bg-emerald-50 text-emerald-800",
  missing: "bg-stone-100 text-stone-600",
  unmanaged: "bg-amber-50 text-amber-800",
  invalid: "bg-red-50 text-red-700",
  error: "bg-red-50 text-red-700",
};

/**
 * A fresh page preselects only the primary first-party CLI, matching the old
 * Codex tab's default. Claude Code is opt-in: it has its own settings file and
 * fixed `/model` slots, so writing it without an explicit choice would surprise
 * users who only wanted their Codex CLI configured.
 */
function defaultSelectedClientKeys(clients: ConfigWriteClientStatus[]): string[] {
  const firstNative = clients.find(
    (client) => client.native && client.platform === DEFAULT_AGENT_PLATFORM,
  );
  return firstNative ? [firstNative.client_key] : [];
}

/**
 * Chinese copy for config-write failures whose backend message does not convey
 * the consequence. Keyed by error code so the wording stays with the reason.
 */
const configWriteErrorMessages: Record<string, string> = {
  "validation.route_config_existing_invalid":
    "现有配置文件无法解析，已拒绝覆盖以免丢失你的 provider 配置。请先修复该文件再重试。",
  "config.concurrent_modification": "配置文件在写入期间被其他程序修改，未做改动。请重试。",
  "config.pool_models_empty": "算力池中没有可用模型，请先向池中加入账号。",
  "config.client_unavailable": "所选客户端不支持当前接口格式。",
  "config.codex_login_missing":
    "已开启「保持 Codex 登录态」，但未找到 Codex 登录文件。请先在 Codex 中登录后再写入。",
  "config.codex_login_not_chatgpt":
    "已开启「保持 Codex 登录态」，但当前 Codex 只有 API Key 登录、没有 ChatGPT 登录态。请先执行 codex login 登录，或关闭该开关。",
  "config.codex_login_unreadable":
    "Codex 登录文件无法读取，已拒绝写入以免配置无法鉴权。请检查该文件后重试。",
};

function formatApiError(error: unknown, fallback: string): string {
  const code = (error as { code?: unknown } | null)?.code;
  const details = (error as { details?: unknown } | null)?.details;
  const message =
    typeof (error as { message?: unknown } | null)?.message === "string"
      ? (error as { message: string }).message
      : "";
  const detailText =
    typeof details === "string" && details.trim() && details !== message ? `（${details}）` : "";
  if (typeof code === "string" && code) return `${fallback}[${code}]${detailText}`;
  return message ? `${message}${detailText}` : fallback;
}

function configWriteErrorCode(error: unknown): string {
  if (!error || typeof error !== "object") return "";
  const outer = (error as { code?: unknown }).code;
  const code = typeof outer === "string" ? outer : "";
  if (code && configWriteErrorMessages[code]) return code;

  const details = (error as { details?: unknown }).details;
  let parsed: unknown = details;
  if (typeof details === "string") {
    try {
      parsed = JSON.parse(details) as unknown;
    } catch {
      parsed = null;
    }
  }
  if (parsed && typeof parsed === "object") {
    const record = parsed as { cause_code?: unknown; targets?: unknown };
    if (typeof record.cause_code === "string" && configWriteErrorMessages[record.cause_code]) {
      return record.cause_code;
    }
    if (Array.isArray(record.targets)) {
      for (const target of record.targets) {
        const targetCode =
          target && typeof target === "object"
            ? (target as { error_code?: unknown }).error_code
            : null;
        if (typeof targetCode === "string" && configWriteErrorMessages[targetCode]) {
          return targetCode;
        }
      }
    }
  }
  return code;
}

function formatConfigWriteError(error: unknown): string {
  const code = configWriteErrorCode(error);
  return configWriteErrorMessages[code] ?? formatApiError(error, "配置写入失败。");
}

/** A client plus the wire format the gateway writes into its config file. */
type UnifiedClient = ConfigWriteClientStatus & {
  interface_format: AgentInterfaceFormat;
};

type WriteGroup = { platform: string; clientKeys: string[] };

/**
 * One page, but two protocol families under the hood: Codex / Chat clients are
 * written by the `codex` adapters, Anthropic Messages clients by the `claude`
 * adapters. Each family has its own adapters, config path shape and access key.
 * Splitting here keeps the backend wire contracts untouched.
 */
function groupsForClients(clients: UnifiedClient[], keys: string[] | null): WriteGroup[] {
  const byKey = new Map(clients.map((client) => [client.client_key, client]));
  const chosen = keys ?? defaultSelectedClientKeys(clients);
  const groups = new Map<string, string[]>();
  for (const key of chosen) {
    const client = byKey.get(key);
    if (!client) continue;
    const platform = platformForInterfaceFormat(client.interface_format);
    groups.set(platform, [...(groups.get(platform) ?? []), key]);
  }
  return [...groups].map(([platform, clientKeys]) => ({ platform, clientKeys }));
}

/**
 * Aggregate versus precise, above the client cards because it decides the model
 * list everywhere: what gets written into a client config, what a client that
 * discovers models over `/v1/models` sees, and which ids the Claude slot dialog
 * may offer. Routing (precise) mode is first and is the fresh-install default.
 */
const modelModeOptions: { mode: RoutePoolModelMode; label: string; hint: string }[] = [
  {
    mode: "precise",
    label: "路由模式",
    hint: "每个 API 账号各出「账号/模型」，请求钉在该账号；官方账号合并为「official/模型」继续轮换。",
  },
  {
    mode: "aggregate",
    label: "聚合模式",
    hint: "同名模型合并成一条，请求在池内轮换。",
  },
];

function ModelModePicker({
  mode,
  disabled,
  onChange,
}: {
  mode: RoutePoolModelMode;
  disabled: boolean;
  onChange: (mode: RoutePoolModelMode) => void;
}) {
  const active = modelModeOptions.find((option) => option.mode === mode) ?? modelModeOptions[0];

  return (
    <div className="grid gap-2 rounded-xl bg-stone-50 px-3.5 py-3">
      <span className="text-[11px] font-semibold text-stone-600">模型清单</span>
      <div
        aria-label="模型清单模式"
        className="flex w-fit rounded-md bg-white p-1 shadow-inner"
        role="radiogroup"
      >
        {modelModeOptions.map((option) => (
          <button
            key={option.mode}
            aria-checked={mode === option.mode}
            className={`rounded px-3 py-1.5 text-xs font-semibold motion-control focus:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 disabled:cursor-not-allowed disabled:opacity-60 ${
              mode === option.mode ? "bg-stone-900 text-white" : "text-stone-600 hover:text-stone-950"
            }`}
            disabled={disabled}
            onClick={() => {
              if (mode !== option.mode) onChange(option.mode);
            }}
            role="radio"
            type="button"
          >
            {option.label}
          </button>
        ))}
      </div>
      <p className="text-[11px] leading-5 text-stone-500">{active.hint}</p>
    </div>
  );
}

export type ClientSetupFocus = {
  /** Legacy deep-link field: the page no longer has platform tabs. */
  platform?: string | null;
  clientKey?: string | null;
};

type ClientSetupScreenProps = {
  focus?: ClientSetupFocus | null;
  onFocusConsumed?: () => void;
  /** Claude Code 槽位弹窗的「去调整映射」：跳到账号页并按来源筛选。 */
  onOpenAccounts?: (provider: string) => void;
};

export function ClientSetupScreen({
  focus = null,
  onFocusConsumed,
  onOpenAccounts,
}: ClientSetupScreenProps) {
  const queryClient = useQueryClient();
  const desktop = isDesktop();
  // `null` until the user picks, so the page defaults to the first-party CLIs.
  const [selection, setSelection] = useState<string[] | null>(null);
  const [focusClientKey, setFocusClientKey] = useState<string | null>(focus?.clientKey ?? null);
  const [deepSeekHarnessPathOverride, setDeepSeekHarnessPathOverride] = useState<string | null>(null);
  const [slotDialogOpen, setSlotDialogOpen] = useState(false);
  const [slotSettingsError, setSlotSettingsError] = useState<string | null>(null);
  const [writeError, setWriteError] = useState<string | null>(null);
  const [outcomes, setOutcomes] = useState<ConfigWriteOutcome[]>([]);

  // 深链：外部入口指定客户端时，只应用一次，之后由用户自由切换。
  useEffect(() => {
    if (!focus) return;
    setFocusClientKey(focus.clientKey ?? null);
    onFocusConsumed?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- focus 对象由 App 每次 new，依赖它即可。
  }, [focus]);

  const capabilitiesQuery = usePlatformCapabilities();
  const configWriteRule = findPlatformCapability(
    capabilitiesQuery.data,
    DEFAULT_AGENT_PLATFORM,
  )?.operations.config_write;
  const configWriteEnabled =
    capabilitiesQuery.isSuccess && Boolean(configWriteRule) && operationEnabled(configWriteRule);
  const configWriteReason = capabilityReason(configWriteRule);
  const capabilityDisabledReason = configWriteEnabled ? null : configWriteReason;

  const settingsQuery = useQuery({ queryKey: ["settings"], queryFn: getSettings });
  /** The old Codex client list plus Claude Code; every other agent appears once. */
  const clientsQuery = useQuery<UnifiedClient[]>({
    queryKey: ["config-write-clients-unified"],
    queryFn: async () => {
      const [codex, claude] = await Promise.all([
        listConfigWriteClients(DEFAULT_AGENT_PLATFORM),
        listConfigWriteClients(MESSAGES_AGENT_PLATFORM),
      ]);
      const claudeCode =
        claude.find((client) => client.client_key === CLAUDE_CODE_CLIENT_KEY) ??
        claude.find((client) => client.client_key === "claude") ??
        claude.find((client) => client.native);
      const merged = [...codex];
      if (claudeCode && !merged.some((client) => client.client_key === claudeCode.client_key)) {
        merged.push(claudeCode);
      }
      return merged.map((client) => ({
        ...client,
        interface_format: defaultAgentInterfaceFormat(client.client_key),
      }));
    },
  });
  const routeProxyQuery = useQuery({
    queryKey: ["route-proxy-status"],
    queryFn: getRouteProxyStatus,
    refetchInterval: 10_000,
  });
  const routeServiceReady =
    Boolean(routeProxyQuery.data?.route_access_enabled) && Boolean(routeProxyQuery.data?.running);
  // 主接入 Key（Codex / Chat 协议族）。Messages 客户端的 Key 由写入流程自己铸。
  const routeProxyKeyQuery = useQuery({
    queryKey: ["route-proxy-key", DEFAULT_AGENT_PLATFORM],
    queryFn: () => getRouteProxyKey(DEFAULT_AGENT_PLATFORM),
    enabled: routeServiceReady,
  });
  const directModesQuery = useQuery({
    queryKey: ["client-direct-modes"],
    queryFn: getClientDirectModes,
    enabled: desktop,
    staleTime: 10_000,
    refetchOnWindowFocus: true,
  });
  const routePoolQuery = useQuery({
    queryKey: ["route-pool", DEFAULT_AGENT_PLATFORM, "active"],
    queryFn: () => getRoutePool(DEFAULT_AGENT_PLATFORM),
    placeholderData: keepPreviousData,
  });
  // 生图没有自己的平台：能否出图只能从账号映射里读出来。
  const allCredentialsQuery = useQuery<RouteCredential[]>({
    queryKey: ["route-credentials-all", DEFAULT_AGENT_PLATFORM],
    queryFn: () => listRouteCredentials(DEFAULT_AGENT_PLATFORM),
    staleTime: 0,
    refetchOnMount: "always",
  });
  const poolImageModels = useMemo(
    () => poolImageModelsOf(allCredentialsQuery.data ?? []),
    [allCredentialsQuery.data],
  );
  const claudeSlotPlanQuery = useQuery({
    queryKey: ["claude-slot-plan", routePoolQuery.data?.model_mode ?? "precise"],
    queryFn: getClaudeSlotPlan,
    enabled: slotDialogOpen,
  });

  const clients = clientsQuery.data ?? [];
  // Login diagnostics belong beside the write option. A failed probe must not
  // block configuration writes; the backend still validates the actual login.
  const codexLoginQuery = useQuery({
    queryKey: ["codex-login-state"],
    queryFn: getCodexLoginState,
    enabled: desktop && clients.some((client) => client.client_key === "codex"),
    retry: false,
  });
  const loginState = codexLoginQuery.data;
  const codexLoginWarnings: Record<string, string> = {
    missing: "未找到 Codex 登录态。请先执行 codex login 登录，否则开启该开关后写入配置会被拒绝。",
    not_chatgpt: "当前 Codex 只有 API Key 登录，没有可保留的 ChatGPT 登录态。请先执行 codex login，或关闭该开关。",
    unreadable: "Codex 登录文件无法读取。开启该开关后写入配置会被拒绝，请先检查该文件。",
  };
  const codexLoginWarning = loginState && !loginState.usable
    ? codexLoginWarnings[loginState.state]
    : null;
  const clientByTargetKey = useMemo(() => {
    const map = new Map<string, ConfigWriteClientStatus>();
    for (const client of clients) map.set(client.target_key, client);
    return map;
  }, [clients]);

  const storedClientSelection = useMemo<string[] | null>(() => {
    const raw = settingsQuery.data?.config_write_clients_json;
    if (!raw) return null;
    try {
      const parsed = JSON.parse(raw) as Record<string, string[]>;
      const keys = [
        ...(Array.isArray(parsed[DEFAULT_AGENT_PLATFORM]) ? parsed[DEFAULT_AGENT_PLATFORM] : []),
        ...(Array.isArray(parsed[MESSAGES_AGENT_PLATFORM]) ? parsed[MESSAGES_AGENT_PLATFORM] : []),
      ];
      return keys.length ? [...new Set(keys)] : null;
    } catch {
      return null;
    }
  }, [settingsQuery.data?.config_write_clients_json]);

  const initialSelection = focusClientKey ? [focusClientKey] : storedClientSelection;
  const selected = selection ?? initialSelection ?? defaultSelectedClientKeys(clients);

  const directClientKeys = useMemo(
    () => (directModesQuery.data ?? []).map((item) => item.client_key),
    [directModesQuery.data],
  );

  const httpEndpoint =
    routeProxyEndpointForPlatform(routeProxyQuery.data?.base_url ?? "", DEFAULT_AGENT_PLATFORM) ||
    null;
  const httpsEndpoint =
    routeProxyEndpointForPlatform(routeProxyQuery.data?.https_base_url ?? "", DEFAULT_AGENT_PLATFORM) ||
    null;
  const imageEndpoint = poolImageModels.length
    ? imageGenerationEndpoint(routeProxyQuery.data?.base_url ?? "", DEFAULT_AGENT_PLATFORM)
    : "";
  const directSelectedClients = clients.filter(
    (client) => selected.includes(client.client_key) && directClientKeys.includes(client.client_key),
  );
  const deepSeekHarnessSelected = selected.some((clientKey) =>
    clientKey.startsWith("deepseek_harness"),
  );
  const deepSeekHarnessPath =
    deepSeekHarnessPathOverride ?? settingsQuery.data?.deepseek_harness_config_path ?? "";

  const toggle = (clientKey: string) => {
    setSelection((current) => {
      const base = current ?? initialSelection ?? defaultSelectedClientKeys(clients);
      return base.includes(clientKey)
        ? base.filter((key) => key !== clientKey)
        : [...base, clientKey];
    });
  };

  const setModelModeMutation = useMutation({
    mutationFn: (mode: RoutePoolModelMode) =>
      setRoutePoolModelMode({ platform: DEFAULT_AGENT_PLATFORM, mode }),
    onSuccess: (state) => {
      queryClient.setQueryData(["route-pool", DEFAULT_AGENT_PLATFORM, "active"], state);
      void queryClient.invalidateQueries({ queryKey: ["route-config-stale"] });
      void queryClient.invalidateQueries({ queryKey: ["claude-slot-plan"] });
    },
    onError: (error) => setWriteError(formatApiError(error, "切换模型清单模式失败。")),
  });

  const saveSlotSettingsMutation = useMutation({
    mutationFn: async (payload: {
      overridesJson: string;
      clientConfigJson: string | null;
      accountSlotMappings?: Array<{ accountId: string; mappingsJson: string }> | null;
    }) => {
      const settings = settingsQuery.data;
      if (!settings) throw new Error("设置尚未加载完成。");
      // Save each touched account's role mappings first (single source of truth),
      // then the slot overrides/client config. All must succeed together.
      for (const mapping of payload.accountSlotMappings ?? []) {
        await updateClaudeAccountSlotMappings(mapping.accountId, mapping.mappingsJson);
      }
      return saveSettings({
        ...settings,
        claude_slot_overrides_json: payload.overridesJson,
        claude_client_config_json: payload.clientConfigJson,
      });
    },
    onSuccess: (settings) => {
      queryClient.setQueryData(["settings"], settings);
      setSlotSettingsError(null);
      setSlotDialogOpen(false);
      void queryClient.invalidateQueries({ queryKey: ["route-config-stale"] });
      void queryClient.invalidateQueries({ queryKey: ["claude-slot-plan"] });
      void queryClient.invalidateQueries({ queryKey: ["route-credentials"] });
    },
    onError: (error) => setSlotSettingsError(formatApiError(error, "保存槽位设置失败。")),
  });

  // Keep the existing persisted preference, with a single entry on the Codex
  // card. Missing settings keep the opt-in off until loading finishes.
  const codexKeepLoginState = settingsQuery.data?.codex_keep_login_state ?? false;
  const saveCodexLoginMutation = useMutation({
    mutationFn: async (enabled: boolean) => {
      const settings = settingsQuery.data;
      if (!settings) throw new Error("设置尚未加载完成。");
      return saveSettings({ ...settings, codex_keep_login_state: enabled });
    },
    onSuccess: (settings) => {
      queryClient.setQueryData(["settings"], settings);
      void queryClient.invalidateQueries({ queryKey: ["route-config-stale"] });
    },
    onError: (error) => setWriteError(formatApiError(error, "保存 Codex 登录态设置失败。")),
  });

  const writeConfigsMutation = useMutation({
    mutationFn: async ({
      clientKeys,
      deepSeekHarnessConfigPath,
    }: {
      clientKeys: string[];
      deepSeekHarnessConfigPath: string | null;
    }) => {
      if (!configWriteEnabled) throw new Error(configWriteReason);
      let settings = settingsQuery.data;
      if (
        deepSeekHarnessConfigPath !== (settings?.deepseek_harness_config_path ?? null) &&
        (settings || deepSeekHarnessConfigPath !== null)
      ) {
        if (!settings) throw new Error("设置尚未加载，无法保存 DSH 配置路径。");
        const pathUpdated = await saveSettings({
          ...settings,
          deepseek_harness_config_path: deepSeekHarnessConfigPath,
        });
        queryClient.setQueryData(["settings"], pathUpdated);
        settings = pathUpdated;
      }

      const groups = groupsForClients(clients, clientKeys);
      const writeOutcomes: ConfigWriteOutcome[] = [];
      for (const group of groups) {
        const results = await writeRouteProxyConfigs(
          routeProxyQuery.data?.base_url ?? null,
          group.platform,
          group.clientKeys,
        );
        writeOutcomes.push(...results);
      }

      if (settings) {
        let existing: Record<string, string[]> = {};
        try {
          existing = settings.config_write_clients_json
            ? (JSON.parse(settings.config_write_clients_json) as Record<string, string[]>)
            : {};
        } catch {
          existing = {};
        }
        const byPlatform = new Map(groups.map((group) => [group.platform, group.clientKeys]));
        const updated = await saveSettings({
          ...settings,
          config_write_clients_json: JSON.stringify({
            ...existing,
            [DEFAULT_AGENT_PLATFORM]: byPlatform.get(DEFAULT_AGENT_PLATFORM) ?? [],
            [MESSAGES_AGENT_PLATFORM]: byPlatform.get(MESSAGES_AGENT_PLATFORM) ?? [],
          }),
        });
        queryClient.setQueryData(["settings"], updated);
      }
      return writeOutcomes;
    },
    onMutate: () => setWriteError(null),
    onSuccess: (writeOutcomes) => {
      setOutcomes(writeOutcomes);
      setFocusClientKey(null);
      const failed = writeOutcomes.filter((outcome) => outcome.status !== "succeeded");
      if (failed.length > 0) {
        const names = failed.map((outcome) => {
          const label =
            clientByTargetKey.get(outcome.target_key)?.display_name ?? outcome.target_key;
          return outcome.error_code ? `${label}（${outcome.error_code}）` : label;
        });
        setWriteError(`以下客户端没有写入成功：${names.join("、")}`);
      }
      void queryClient.invalidateQueries({ queryKey: ["client-direct-modes"] });
      void queryClient.invalidateQueries({ queryKey: ["route-config-stale"] });
      void queryClient.invalidateQueries({ queryKey: ["config-write-clients-unified"] });
    },
    onError: (error) => {
      setWriteError(formatConfigWriteError(error));
      void queryClient.invalidateQueries({ queryKey: ["client-direct-modes"] });
    },
  });

  const refreshClientState = () => {
    void queryClient.invalidateQueries({ queryKey: ["route-config-stale"] });
    void queryClient.invalidateQueries({ queryKey: ["config-write-clients-unified"] });
  };

  const rollbackMutation = useMutation({
    mutationFn: rollbackConfigSnapshot,
    onMutate: () => setWriteError(null),
    onSuccess: (outcome) => {
      setOutcomes((current) => [...current, outcome]);
      refreshClientState();
    },
    onError: (error) => setWriteError(formatApiError(error, "回滚配置失败。")),
  });

  const restoreNativeMutation = useMutation({
    mutationFn: restoreNativeConfig,
    onMutate: () => setWriteError(null),
    onSuccess: (outcome) => {
      setOutcomes((current) => [...current, outcome]);
      refreshClientState();
    },
    onError: (error) => setWriteError(formatApiError(error, "还原原生配置失败。")),
  });

  const configWriteStaleQuery = useQuery({
    queryKey: [
      "route-config-stale",
      routeProxyQuery.data?.base_url ?? null,
      allCredentialsQuery.dataUpdatedAt,
      settingsQuery.data?.claude_client_config_json ?? null,
      settingsQuery.data?.claude_slot_overrides_json ?? null,
      storedClientSelection,
      routePoolQuery.data?.model_mode ?? null,
    ],
    queryFn: async () => {
      const groups = groupsForClients(clients, storedClientSelection);
      const results = await Promise.all(
        groups.map((group) =>
          routeConfigWriteIsStale(
            routeProxyQuery.data?.base_url ?? null,
            group.platform,
            group.clientKeys,
          ),
        ),
      );
      return results.some(Boolean);
    },
    enabled: routeServiceReady && configWriteEnabled,
    staleTime: 0,
  });
  const configWriteStale = configWriteStaleQuery.data === true;

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!configWriteEnabled || writeConfigsMutation.isPending || saveCodexLoginMutation.isPending || selected.length === 0) return;
    const customDeepSeekHarnessPath = deepSeekHarnessPath.trim();
    writeConfigsMutation.mutate({
      clientKeys: clients
        .map((client) => client.client_key)
        .filter((clientKey) => selected.includes(clientKey)),
      deepSeekHarnessConfigPath: customDeepSeekHarnessPath ? customDeepSeekHarnessPath : null,
    });
  };

  const writing = writeConfigsMutation.isPending;

  const renderClientCard = (client: UnifiedClient): ReactNode => {
    const checked = selected.includes(client.client_key);
    const direct = directClientKeys.includes(client.client_key);
    const isClaudeCode =
      client.interface_format === "messages" &&
      (client.client_key === CLAUDE_CODE_CLIENT_KEY || client.client_key === "claude");
    return (
      <li
        className={`flex items-center gap-3 px-3.5 py-2.5 motion-control ${
          checked ? "bg-stone-50" : "bg-white hover:bg-stone-50/60"
        }`}
        data-testid={`client-card-${client.client_key}`}
        key={client.client_key}
      >
        <input
          aria-label={`写入 ${client.display_name}`}
          checked={checked}
          className="h-4 w-4 shrink-0 accent-stone-900"
          disabled={Boolean(capabilityDisabledReason) || writing}
          onChange={() => toggle(client.client_key)}
          type="checkbox"
        />
        <ClientIcon className="h-8 w-8 shrink-0" clientKey={client.client_key} />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <span className="flex items-center gap-1 text-[13px] font-semibold text-stone-950">
              {client.display_name}
              {client.client_key === "codex" ? (
                <span
                  aria-label="Codex CLI、ChatGPT 桌面版、VS Code 扩展、Codeg 等都会读取这个官方路径的配置，通常只需配置这一个。"
                  className="inline-flex"
                  title="Codex CLI、ChatGPT 桌面版、VS Code 扩展、Codeg 等都会读取这个官方路径的配置，通常只需配置这一个。"
                >
                  <HelpCircle aria-hidden="true" className="h-3.5 w-3.5 text-stone-400" />
                </span>
              ) : null}
            </span>
            <span
              aria-label={`接口格式 ${interfaceFormatLabels[client.interface_format]}`}
              className="rounded-full bg-indigo-50 px-2 py-0.5 text-[11px] font-semibold text-indigo-800"
              title={interfaceFormatHints[client.interface_format]}
            >
              {interfaceFormatLabels[client.interface_format]}
            </span>
            <span
              className={`rounded-full px-2 py-0.5 text-[11px] font-semibold ${
                fileStatusTones[client.file_status] ?? "bg-stone-100 text-stone-600"
              }`}
            >
              {fileStatusLabels[client.file_status] ?? client.file_status}
            </span>
            {client.native ? (
              <span className="rounded-full bg-sky-50 px-2 py-0.5 text-[11px] font-semibold text-sky-800">
                内置
              </span>
            ) : null}
            {direct ? (
              <span className="rounded-full bg-violet-50 px-2 py-0.5 text-[11px] font-semibold text-violet-800">
                直连中
              </span>
            ) : null}
          </div>
          {client.config_path ? (
            <div
              className="mt-0.5 truncate font-mono text-[11px] text-stone-400"
              title={client.config_path}
            >
              {client.config_path}
            </div>
          ) : null}
          {client.client_key === "codex" ? (
            <div className="mt-2 flex flex-wrap items-center gap-2">
              <label
                className="inline-flex cursor-pointer items-center gap-1.5 text-[11px] font-semibold text-stone-700"
                title="写入配置时让 Codex 继续使用自己的 ChatGPT 登录，保留联网搜索等仅登录可用的功能；需要 Codex 已登录。"
              >
                <input
                  aria-label="保持 Codex 登录态"
                  checked={codexKeepLoginState}
                  className="h-3.5 w-3.5 shrink-0 accent-stone-900"
                  disabled={!settingsQuery.data || writing || saveCodexLoginMutation.isPending}
                  onChange={(event) => saveCodexLoginMutation.mutate(event.target.checked)}
                  type="checkbox"
                />
                保持 Codex 登录态
                <span className="font-normal text-stone-400">（保留联网搜索，默认关闭）</span>
              </label>
              {codexLoginWarning ? (
                <p
                  className="w-full rounded-lg border border-amber-200 bg-amber-50/70 px-2.5 py-1.5 text-[11px] leading-4 text-amber-900"
                  role="status"
                >
                  {codexLoginWarning}
                </p>
              ) : null}
            </div>
          ) : null}
          {client.error_code ? (
            <div className="mt-0.5 font-mono text-[11px] text-red-700">{client.error_code}</div>
          ) : null}
        </div>
        {isClaudeCode ? (
          <button
            aria-label="设置 Claude Code 槽位映射"
            className="shrink-0 rounded-md border border-stone-200 bg-white px-2.5 py-1.5 text-[12px] font-semibold text-stone-700 motion-control hover:bg-stone-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-blue-500"
            onClick={() => {
              setSlotSettingsError(null);
              setSlotDialogOpen(true);
            }}
            type="button"
          >
            设置
          </button>
        ) : null}
      </li>
    );
  };

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      <form className="flex min-h-0 flex-1 flex-col" onSubmit={submit}>
        <div
          className="min-h-0 flex-1 overflow-y-auto overscroll-contain"
          data-testid="client-setup-scroll-region"
        >
          <div className="mx-auto w-full max-w-5xl px-5 py-6">
            <header className="flex flex-wrap items-center justify-between gap-4 rounded-2xl border border-stone-200 bg-gradient-to-br from-white to-stone-50 px-5 py-4 shadow-sm">
              <div className="flex items-center gap-3">
                <span className="grid h-11 w-11 shrink-0 place-items-center rounded-2xl bg-stone-900 text-white shadow-sm">
                  <Plug aria-hidden="true" className="h-5 w-5" />
                </span>
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <h1 className="text-lg font-semibold tracking-tight text-stone-950">智能体</h1>
                    <span className="rounded-full bg-stone-100 px-2 py-0.5 text-[10px] font-semibold text-stone-600">
                      {clients.length} 个可接入
                    </span>
                  </div>
                  <p className="mt-0.5 text-[12px] text-stone-500">
                    选择目标，一键把当前算力池写入它们的配置。
                  </p>
                </div>
              </div>
              <div className="flex items-center gap-2 rounded-full border border-stone-200 bg-white px-3 py-1.5 text-[11px] font-semibold text-stone-600">
                <span
                  className={`h-1.5 w-1.5 rounded-full ${routeServiceReady ? "bg-emerald-500" : "bg-stone-400"}`}
                />
                {routeServiceReady ? "网关就绪" : "网关未启动"}
              </div>
            </header>

            {configWriteStale ? (
              <p
                className="mt-4 rounded-xl border border-amber-200 bg-amber-50 px-3 py-2 text-[12px] font-semibold text-amber-950"
                role="status"
              >
                配置已变更，需重新写入才会生效。
              </p>
            ) : null}

            {writeError ? (
              <div
                className="mt-4 flex items-start justify-between gap-3 rounded-xl border border-red-200 bg-red-50 px-3 py-2 text-[12px] font-semibold text-red-700"
                role="alert"
              >
                <p>{writeError}</p>
                <button
                  aria-label="关闭配置写入错误"
                  className="text-red-400 motion-control hover:text-red-700"
                  onClick={() => setWriteError(null)}
                  type="button"
                >
                  ✕
                </button>
              </div>
            ) : null}

            {outcomes.length > 0 ? (
              <details className="mt-4 rounded-xl border border-stone-200 bg-stone-50 px-3 py-2">
                <summary className="flex cursor-pointer items-center justify-between gap-3 text-[12px] font-semibold text-stone-700">
                  <span>配置写入结果</span>
                  <button
                    aria-label="关闭配置写入结果"
                    className="text-stone-400 motion-control hover:text-stone-700"
                    onClick={(event) => {
                      event.preventDefault();
                      event.stopPropagation();
                      setOutcomes([]);
                    }}
                    type="button"
                  >
                    ✕
                  </button>
                </summary>
                <div className="mt-2 space-y-1 text-[12px] text-stone-600">
                  {outcomes.map((outcome) => (
                    <div
                      className="rounded-lg border border-stone-200 bg-white px-2.5 py-2"
                      key={`${outcome.operation_id}:${outcome.target_key}:${outcome.snapshot_id ?? "none"}`}
                    >
                      <p>
                        {clientByTargetKey.get(outcome.target_key)?.display_name ?? outcome.target_key} ·{" "}
                        {outcome.platform}: {outcome.path || "未解析路径"} ({outcome.status})
                      </p>
                      <p className="mt-1 font-mono text-[11px] text-stone-500">
                        operation {outcome.operation_id} · snapshot {outcome.snapshot_id ?? "none"}
                      </p>
                      {outcome.error_code ? (
                        <p className="mt-1 font-mono text-[11px] text-red-600">{outcome.error_code}</p>
                      ) : null}
                      {outcome.status === "succeeded" && outcome.snapshot_id ? (
                        <button
                          aria-label={`回滚此次写入 ${outcome.target_key}`}
                          className="mt-2 rounded-lg border border-stone-300 bg-white px-2 py-1 text-[11px] font-semibold text-stone-700 motion-control hover:bg-stone-50 disabled:opacity-40"
                          disabled={rollbackMutation.isPending}
                          onClick={() => rollbackMutation.mutate(outcome.snapshot_id as string)}
                          type="button"
                        >
                          回滚此次写入
                        </button>
                      ) : null}
                    </div>
                  ))}
                  <div className="flex flex-wrap items-center gap-2 border-t border-stone-200 pt-2">
                    <span className="text-[11px] font-semibold text-stone-500">还原原生配置：</span>
                    {selected.map((clientKey) => {
                      const targetKey = clientByTargetKey.get(clientKey)?.target_key;
                      if (!targetKey) return null;
                      return (
                        <button
                          aria-label={`还原 ${clientKey} 的原生配置`}
                          className="rounded-lg border border-stone-300 bg-white px-2 py-1 text-[11px] font-semibold text-stone-700 motion-control hover:bg-stone-50 disabled:opacity-40"
                          disabled={restoreNativeMutation.isPending}
                          key={clientKey}
                          onClick={() => restoreNativeMutation.mutate(targetKey)}
                          type="button"
                        >
                          {clientByTargetKey.get(clientKey)?.display_name ?? clientKey}
                        </button>
                      );
                    })}
                  </div>
                </div>
              </details>
            ) : null}

            <div className="mt-5 grid items-start gap-4 lg:grid-cols-[minmax(0,1fr)_300px]">
              <section className="rounded-2xl border border-stone-200 bg-white p-4 shadow-sm">
                <div className="mb-3 flex items-center justify-between gap-3">
                  <div>
                    <h2 className="text-[15px] font-semibold text-stone-950">接入目标</h2>
                    <p className="mt-0.5 text-[11px] text-stone-500">
                      已选 {selected.length} 个 · 接口格式决定写入适配器与接入 Key
                    </p>
                  </div>
                  <span className="rounded-full bg-stone-900 px-2.5 py-1 text-[10px] font-semibold text-white">
                    {selected.length} / {clients.length}
                  </span>
                </div>

                {capabilityDisabledReason ? (
                  <p
                    className="mb-3 rounded-xl border border-amber-200 bg-amber-50 px-3 py-2 text-[11px] leading-5 text-amber-950"
                    role="alert"
                  >
                    {capabilityDisabledReason}
                  </p>
                ) : null}

                {clientsQuery.isLoading ? (
                  <p className="text-[12px] text-stone-500">读取客户端列表中...</p>
                ) : clients.length === 0 ? (
                  <p className="rounded-xl bg-stone-50 px-3.5 py-3 text-xs leading-5 text-stone-600">
                    暂无可写入的客户端。
                  </p>
                ) : (
                  <ul className="list-none divide-y divide-stone-100 overflow-hidden rounded-xl border border-stone-200 bg-white">
                    {clients.map(renderClientCard)}
                  </ul>
                )}

                {directSelectedClients.length > 0 ? (
                  <p
                    className="mt-3 rounded-xl border border-amber-200 bg-amber-50 px-3 py-2 text-[11px] leading-5 text-amber-950"
                    role="note"
                  >
                    写入后将结束{" "}
                    {directSelectedClients.map((client) => client.display_name).join("、")}
                    {" "}的直连模式，切回算力池。
                  </p>
                ) : null}

                {deepSeekHarnessSelected ? (
                  <details className="mt-3 rounded-xl border border-stone-200 bg-stone-50 px-3 py-2">
                    <summary className="cursor-pointer text-[11px] font-semibold text-stone-600">
                      高级：DeepSeek Harness 配置路径
                    </summary>
                    <input
                      aria-label="DSH 配置文件路径"
                      className="mt-2 h-8 w-full rounded-md border border-stone-200 bg-white px-2.5 font-mono text-[11px] text-stone-800 outline-none placeholder:font-sans placeholder:text-stone-400 focus:border-blue-400"
                      disabled={Boolean(capabilityDisabledReason) || writing}
                      onChange={(event) => setDeepSeekHarnessPathOverride(event.target.value)}
                      placeholder={
                        selected.includes("deepseek_harness_desktop")
                          ? "留空使用 ~/.dsh/profiles/desktop/cordis.patch.yml"
                          : "留空使用 ~/.dsh/settings.yaml"
                      }
                      type="text"
                      value={deepSeekHarnessPath}
                    />
                  </details>
                ) : null}
              </section>

              <aside className="grid gap-4">
                <section className="rounded-2xl border border-stone-200 bg-white p-4 shadow-sm">
                  <h2 className="mb-3 text-[13px] font-semibold text-stone-950">路由模式</h2>
                  <ModelModePicker
                    disabled={writing || setModelModeMutation.isPending}
                    mode={routePoolQuery.data?.model_mode ?? "precise"}
                    onChange={(mode) => setModelModeMutation.mutate(mode)}
                  />
                </section>

                <details className="rounded-2xl border border-stone-200 bg-white p-4 shadow-sm">
                  <summary className="cursor-pointer text-[13px] font-semibold text-stone-950">
                    手动接入参数
                  </summary>
                  <div className="mt-3 grid gap-2.5">
                    <EndpointRow label="Base URL" value={httpEndpoint} />
                    {httpsEndpoint ? <EndpointRow label="HTTPS Base URL" value={httpsEndpoint} /> : null}
                    <EndpointRow label="API Key" secret value={routeServiceReady ? routeProxyKeyQuery.data ?? null : null} />
                    {imageEndpoint ? <EndpointRow label="图片端点" value={imageEndpoint} /> : null}
                    <p className="text-[11px] leading-5 text-stone-500">
                    {httpsEndpoint ? (
                      <p className="text-[11px] leading-5 text-stone-500">
                        HTTPS 端点仅在需要 TLS 时使用，客户端必须信任本地根证书。
                      </p>
                    ) : null}
                    {imageEndpoint ? (
                      <details className="rounded-lg border border-stone-100 bg-stone-50 px-2.5 py-2">
                        <summary className="cursor-pointer text-[11px] font-semibold text-stone-600">
                          可用生图模型
                        </summary>
                        <p className="mt-1 text-[11px] leading-5 text-stone-500">
                          图片端点走同一个池、同一张 key。可用的生图模型：{poolImageModels.join("、")}。
                        </p>
                      </details>
                    ) : null}
                      内置支持之外的 Agent 可手动填写这些参数；主 Key 覆盖 Codex / Chat 协议族。
                    </p>
                  </div>
                </details>
              </aside>
            </div>
          </div>
        </div>

        <footer className="shrink-0 border-t border-stone-200 bg-stone-100">
          <div
            aria-label="配置写入操作"
            className="mx-auto flex w-full max-w-5xl shrink-0 items-center justify-between gap-3 px-5 py-3"
            role="group"
          >
            <div className="flex min-w-0 items-center gap-2">
              <span className="grid h-6 w-6 shrink-0 place-items-center rounded-full bg-stone-900 text-[10px] font-semibold text-white">
                {selected.length}
              </span>
              <span className="truncate text-[11px] text-stone-500">
                只写入勾选的客户端，其他客户端的配置文件不会被改动。
              </span>
            </div>
            <button
              className="h-10 shrink-0 rounded-xl bg-stone-900 px-5 text-sm font-semibold text-white shadow-sm motion-control hover:bg-stone-800 focus:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 disabled:cursor-not-allowed disabled:opacity-60"
              disabled={Boolean(capabilityDisabledReason) || writing || saveCodexLoginMutation.isPending || selected.length === 0}
              type="submit"
            >
              {writing ? "写入中..." : "一键写入"}
            </button>
          </div>
        </footer>
      </form>

      {slotDialogOpen ? (
        <ClaudeSlotSettingsDialog
          clientConfigJson={settingsQuery.data?.claude_client_config_json}
          error={slotSettingsError}
          onClose={() => setSlotDialogOpen(false)}
          onOpenAccounts={() => onOpenAccounts?.("claude")}
          onSave={(payload) => saveSlotSettingsMutation.mutate(payload)}
          overridesJson={settingsQuery.data?.claude_slot_overrides_json}
          plan={claudeSlotPlanQuery.data}
          saving={saveSlotSettingsMutation.isPending}
        />
      ) : null}
    </div>
  );
}
