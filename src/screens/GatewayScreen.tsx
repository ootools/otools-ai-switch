import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Copy, List, Network, Play, RefreshCw, ScrollText, Send, Square, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { EndpointRow } from "../components/accounts/EndpointRow";
import { MotionDialog } from "../components/motion/MotionPrimitives";
import {
  getRouteProxyKey,
  getRouteProxyStatus,
  routePoolTestModel,
  startRouteProxy,
  stopRouteProxy,
  subscribeRouteProxyLiveLog,
  unsubscribeRouteProxyLiveLog,
} from "../lib/api/client";
import type { RoutePoolModelTestOutcome, RouteProxyLiveLogEntry } from "../lib/api/types";
import { copyPlainText } from "../lib/copyToClipboard";
import { useI18n } from "../lib/i18n";
import {
  imageGenerationEndpoint,
  routeProxyApiEndpoint,
  routeProxyEndpointForPlatform,
} from "../lib/routeProxyEndpoint";
import { fetchRouteProxyModels } from "../lib/routeProxyModels";

const PLATFORM = "codex";
const cardClass = "space-y-4 rounded-2xl border border-stone-200 bg-white/82 p-4 shadow-sm";
const buttonClass =
  "inline-flex items-center gap-1.5 rounded-xl border border-stone-300 bg-white px-3 py-2 text-[12px] font-semibold text-stone-700 motion-control hover:bg-stone-50 disabled:cursor-not-allowed disabled:opacity-50";
const primaryClass =
  "inline-flex items-center gap-1.5 rounded-xl bg-stone-900 px-3 py-2 text-[12px] font-semibold text-white shadow-sm motion-control hover:bg-stone-800 disabled:cursor-not-allowed disabled:opacity-50";

function CopyPill({
  label,
  value,
  copiedKey,
  onCopy,
}: {
  label: string;
  value: string;
  copiedKey: string | null;
  onCopy: (value: string) => void;
}) {
  return (
    <button
      aria-label={`复制 ${label}`}
      className="group flex min-w-0 items-center gap-2 rounded-xl border border-stone-200 bg-stone-50/70 px-3 py-2 text-left motion-control hover:border-stone-300 hover:bg-white"
      onClick={() => onCopy(value)}
      title={value}
      type="button"
    >
      <span className="min-w-0 flex-1">
        <span className="block text-[10px] font-semibold uppercase tracking-wide text-stone-400">
          {label}
        </span>
        <span className="mt-0.5 block truncate font-mono text-[11px] text-stone-800">
          {value}
        </span>
      </span>
      {copiedKey === value ? (
        <Check aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-emerald-600" />
      ) : (
        <Copy aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-stone-400 group-hover:text-stone-700" />
      )}
    </button>
  );
}

function LogDetailRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="grid gap-0.5 border-b border-stone-100 py-2 last:border-0">
      <dt className="text-[10px] font-semibold uppercase tracking-wide text-stone-400">{label}</dt>
      <dd className="whitespace-pre-wrap break-all font-mono text-[11px] text-stone-800">
        {value}
      </dd>
    </div>
  );
}

/** 网关独立页面：状态、端点、模型测试、模型清单与实时日志。 */
export function GatewayScreen() {
  const queryClient = useQueryClient();
  const { t } = useI18n();
  const [model, setModel] = useState("");
  const [toolCall, setToolCall] = useState(false);
  const [outcome, setOutcome] = useState<RoutePoolModelTestOutcome | null>(null);
  const [testError, setTestError] = useState<string | null>(null);
  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const [selectedLog, setSelectedLog] = useState<RouteProxyLiveLogEntry | null>(null);

  const statusQuery = useQuery({
    queryKey: ["gateway-status"],
    queryFn: getRouteProxyStatus,
    refetchInterval: 5000,
  });
  const keyQuery = useQuery({
    queryKey: ["gateway-api-key"],
    queryFn: () => getRouteProxyKey(PLATFORM),
  });
  const startMutation = useMutation({
    mutationFn: startRouteProxy,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["gateway-status"] }),
  });
  const stopMutation = useMutation({
    mutationFn: stopRouteProxy,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["gateway-status"] }),
  });
  const modelsQuery = useQuery({
    queryKey: ["gateway-models"],
    queryFn: () => fetchRouteProxyModels(PLATFORM),
  });
  const logsQuery = useQuery({
    queryKey: ["gateway-live-log"],
    queryFn: () => subscribeRouteProxyLiveLog(PLATFORM),
    refetchInterval: 3000,
  });

  useEffect(() => () => void unsubscribeRouteProxyLiveLog().catch(() => undefined), []);

  const testMutation = useMutation({
    mutationFn: () =>
      routePoolTestModel({
        platform: PLATFORM,
        model: model.trim() || null,
        test_tool_call: toolCall,
      }),
    onSuccess: (result) => {
      setOutcome(result);
      setTestError(null);
    },
    onError: (error) => {
      setOutcome(null);
      setTestError(error instanceof Error ? error.message : "模型测试失败。");
    },
  });

  const status = statusQuery.data;
  const running = status?.running ?? false;
  const busy = startMutation.isPending || stopMutation.isPending;
  const baseUrl = status?.base_url ?? "";
  const codexBase = routeProxyEndpointForPlatform(baseUrl, PLATFORM);

  const endpoints = useMemo(
    () =>
      [
        { label: "Base URL", value: codexBase },
        { label: "Chat Completions", value: routeProxyApiEndpoint(codexBase, "/chat/completions") },
        { label: "Responses", value: routeProxyApiEndpoint(codexBase, "/responses") },
        { label: "Anthropic Messages", value: routeProxyApiEndpoint(baseUrl, "/v1/messages") },
        { label: "Images", value: imageGenerationEndpoint(baseUrl, PLATFORM) },
      ].filter((item) => Boolean(item.value)),
    [baseUrl, codexBase],
  );

  const copyValue = async (value: string) => {
    if (!value) return;
    await copyPlainText(value);
    setCopiedKey(value);
    window.setTimeout(() => setCopiedKey(null), 1400);
  };

  return (
    <section className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-stone-200 bg-white/82 px-4 py-3 shadow-sm">
        <div className="flex items-center gap-3">
          <span className="grid h-10 w-10 shrink-0 place-items-center rounded-xl bg-stone-900 text-white">
            <Network aria-hidden="true" className="h-5 w-5" />
          </span>
          <div>
            <h1 className="text-lg font-semibold tracking-tight text-stone-950">网关</h1>
            <p className="mt-0.5 flex items-center gap-1.5 text-[12px] text-stone-500">
              <span
                className={`h-1.5 w-1.5 rounded-full ${running ? "bg-emerald-500" : "bg-stone-400"}`}
              />
              {running ? "运行中" : "未运行"}
              <span className="text-stone-300">·</span>
              {status?.route_access_enabled ? "算力池路由已启用" : "算力池路由未启用"}
            </p>
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {running ? (
            <button
              aria-label="停止 AI 服务"
              className="inline-flex items-center gap-1.5 rounded-xl border border-red-700 bg-red-600 px-3 py-2 text-[12px] font-semibold text-white motion-control hover:bg-red-700 disabled:opacity-50"
              disabled={busy}
              onClick={() => stopMutation.mutate()}
              type="button"
            >
              <Square aria-hidden="true" className="h-3.5 w-3.5 fill-current" />
              停止
            </button>
          ) : (
            <button
              aria-label="启动 AI 服务"
              className="inline-flex items-center gap-1.5 rounded-xl border border-emerald-700 bg-emerald-600 px-3 py-2 text-[12px] font-semibold text-white motion-control hover:bg-emerald-700 disabled:opacity-50"
              disabled={busy}
              onClick={() => startMutation.mutate()}
              type="button"
            >
              <Play aria-hidden="true" className="h-3.5 w-3.5 fill-current" />
              启动
            </button>
          )}
          <button
            aria-label="刷新网关状态"
            className={buttonClass}
            disabled={statusQuery.isFetching}
            onClick={() => void queryClient.invalidateQueries({ queryKey: ["gateway-status"] })}
            type="button"
          >
            <RefreshCw
              aria-hidden="true"
              className={`h-3.5 w-3.5 ${statusQuery.isFetching ? "animate-spin" : ""}`}
            />
            刷新
          </button>
        </div>
      </div>

      <div className={cardClass}>
        <div className="flex items-center justify-between gap-2">
          <div>
            <h2 className="text-[15px] font-semibold text-stone-950">连接端点</h2>
            <p className="mt-0.5 text-[11px] text-stone-500">点击任意端点即可复制。</p>
          </div>
          {status?.https_base_url ? (
            <CopyPill
              copiedKey={copiedKey}
              label="HTTPS Base URL"
              onCopy={(value) => void copyValue(value)}
              value={status.https_base_url}
            />
          ) : null}
        </div>
        <div className="grid gap-2 sm:grid-cols-2">
          {endpoints.map((endpoint) => (
            <CopyPill
              copiedKey={copiedKey}
              key={endpoint.label}
              label={endpoint.label}
              onCopy={(value) => void copyValue(value)}
              value={endpoint.value}
            />
          ))}
        </div>
        <div className="rounded-xl border border-stone-200 bg-stone-50/60 px-3 py-2.5">
          <EndpointRow
            label="API Key"
            secret
            value={keyQuery.data ?? null}
          />
        </div>
        {statusQuery.isError ? (
          <p className="text-[12px] text-red-700" role="alert">读取网关状态失败。</p>
        ) : null}
      </div>

      <div className={cardClass}>
        <div className="flex items-center justify-between gap-2">
          <h2 className="text-[15px] font-semibold text-stone-950">模型测试</h2>
          <span className="rounded-full bg-stone-100 px-2 py-0.5 text-[10px] font-semibold text-stone-600">
            真实生成请求
          </span>
        </div>
        <div className="flex flex-wrap items-end gap-3">
          <label className="flex min-w-56 flex-1 flex-col gap-1.5 text-[12px] font-semibold text-stone-600">
            测试模型（可选）
            <input
              aria-label="网关测试模型"
              className="rounded-xl border border-stone-200 bg-white px-3 py-2 text-[13px] font-medium text-stone-900 outline-none focus:border-blue-400"
              list="gateway-model-options"
              onChange={(event) => setModel(event.target.value)}
              placeholder="留空使用默认模型"
              value={model}
            />
            <datalist id="gateway-model-options">
              {(modelsQuery.data ?? []).map((item) => (
                <option key={item.id} value={item.id} />
              ))}
            </datalist>
          </label>
          <label className="flex items-center gap-2 pb-2 text-[12px] font-medium text-stone-700">
            <input
              aria-label="测试工具调用能力"
              checked={toolCall}
              className="accent-stone-900"
              onChange={(event) => setToolCall(event.target.checked)}
              type="checkbox"
            />
            测试工具调用
          </label>
          <button
            aria-label="模型测试"
            className={primaryClass}
            disabled={!running || testMutation.isPending}
            onClick={() => testMutation.mutate()}
            title={!running ? "请先启动网关" : undefined}
            type="button"
          >
            <Send aria-hidden="true" className="h-3.5 w-3.5" />
            {testMutation.isPending ? "测试中…" : "模型测试"}
          </button>
        </div>
        {testError ? <p className="text-[12px] text-red-700" role="alert">{testError}</p> : null}
        {outcome ? (
          <div className="rounded-xl border border-stone-200 bg-stone-50 px-3 py-2 text-[12px] text-stone-700">
            <p className="font-semibold text-stone-900">
              {outcome.selected_account_name} · {outcome.interface_format}
            </p>
            <p className="mt-1 break-all font-mono text-[11px]">
              请求：{outcome.request_path}
              {outcome.target_url ? ` → ${outcome.target_url}` : ""}
            </p>
            <p className="mt-1 text-[11px] text-stone-500">
              {outcome.duration_ms} ms · {outcome.success ? "成功" : "失败"}
              {outcome.response_status ? ` · HTTP ${outcome.response_status}` : ""}
            </p>
            {outcome.error_message ? (
              <p className="mt-1 break-all font-mono text-[11px] text-red-700">{outcome.error_message}</p>
            ) : null}
          </div>
        ) : null}
      </div>

      <div className={cardClass}>
        <div className="flex items-center justify-between gap-2">
          <h2 className="flex items-center gap-1.5 text-[15px] font-semibold text-stone-950">
            <List aria-hidden="true" className="h-4 w-4" />
            模型清单
          </h2>
          <button
            aria-label="刷新模型清单"
            className={buttonClass}
            disabled={modelsQuery.isFetching}
            onClick={() => void queryClient.invalidateQueries({ queryKey: ["gateway-models"] })}
            type="button"
          >
            <RefreshCw
              aria-hidden="true"
              className={`h-3.5 w-3.5 ${modelsQuery.isFetching ? "animate-spin" : ""}`}
            />
            刷新
          </button>
        </div>
        {modelsQuery.isError ? (
          <p className="text-[12px] text-red-700" role="alert">获取模型列表失败。</p>
        ) : modelsQuery.data && modelsQuery.data.length > 0 ? (
          <ul className="grid list-none gap-2 sm:grid-cols-2 xl:grid-cols-3">
            {modelsQuery.data.map((item) => (
              <li
                className="group flex min-w-0 items-center gap-2 rounded-xl border border-stone-200 bg-white px-3 py-2.5 motion-control hover:border-stone-300 hover:bg-stone-50"
                key={item.id}
              >
                <span className="grid h-8 w-8 shrink-0 place-items-center rounded-lg bg-stone-900 font-mono text-[11px] font-semibold text-white">
                  {item.id.slice(0, 2).toUpperCase()}
                </span>
                <span className="min-w-0 flex-1">
                  <span className="block truncate font-mono text-[12px] font-semibold text-stone-900" title={item.id}>
                    {item.id}
                  </span>
                  <span className="mt-0.5 block truncate text-[10px] text-stone-500">
                    {item.owned_by || "算力池"}
                    {item.context_window ? ` · ${Math.round(item.context_window / 1000)}K 上下文` : ""}
                  </span>
                </span>
                <button
                  aria-label={`复制 ${item.id}`}
                  className="grid h-7 w-7 shrink-0 place-items-center rounded-md text-stone-400 motion-control hover:bg-stone-100 hover:text-stone-700"
                  onClick={() => void copyValue(item.id)}
                  title="复制模型 ID"
                  type="button"
                >
                  {copiedKey === item.id ? (
                    <Check aria-hidden="true" className="h-3.5 w-3.5 text-emerald-600" />
                  ) : (
                    <Copy aria-hidden="true" className="h-3.5 w-3.5" />
                  )}
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-[12px] text-stone-500">暂无可用模型。</p>
        )}
      </div>

      <div className={cardClass}>
        <div className="flex items-center justify-between gap-2">
          <h2 className="flex items-center gap-1.5 text-[15px] font-semibold text-stone-950">
            <ScrollText aria-hidden="true" className="h-4 w-4" />
            实时日志
          </h2>
          <button
            aria-label="刷新实时日志"
            className={buttonClass}
            disabled={logsQuery.isFetching}
            onClick={() => void queryClient.invalidateQueries({ queryKey: ["gateway-live-log"] })}
            type="button"
          >
            <RefreshCw
              aria-hidden="true"
              className={`h-3.5 w-3.5 ${logsQuery.isFetching ? "animate-spin" : ""}`}
            />
            刷新
          </button>
        </div>
        {(logsQuery.data ?? []).length === 0 ? (
          <p className="text-[12px] text-stone-500">暂无请求记录。</p>
        ) : (
          <ul className="max-h-72 list-none divide-y divide-stone-100 overflow-y-auto rounded-xl border border-stone-200 bg-white">
            {logsQuery.data?.map((entry: RouteProxyLiveLogEntry) => (
              <li key={entry.id}>
                <button
                  aria-label={`查看日志详情 ${entry.requested_model ?? entry.path}`}
                  className="w-full px-3 py-2 text-left motion-control hover:bg-stone-50"
                  onClick={() => setSelectedLog(entry)}
                  type="button"
                >
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="font-mono text-[12px] font-semibold text-stone-800">
                      {entry.requested_model ?? entry.path}
                    </span>
                    <span className={entry.success ? "text-emerald-700" : "text-red-700"}>
                      {entry.status ?? (entry.success ? "成功" : "失败")}
                    </span>
                    <span className="text-stone-500">{entry.duration_ms} ms</span>
                    <span className="ml-auto text-[10px] text-stone-400">查看详情 ›</span>
                  </div>
                  <p className="mt-0.5 truncate text-[11px] text-stone-500">
                    {entry.credential_name} · {entry.client_key ?? "unknown"} · {entry.path}
                  </p>
                  {entry.error_message ? (
                    <p className="mt-0.5 break-all font-mono text-[11px] text-red-700">
                      {entry.error_message}
                    </p>
                  ) : null}
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>

      <MotionDialog
        ariaLabel="实时日志详情"
        className="w-full max-w-3xl rounded-2xl border border-stone-200 bg-white p-5 shadow-2xl"
        onClose={() => setSelectedLog(null)}
        open={Boolean(selectedLog)}
      >
        {selectedLog ? (
          <div className="flex max-h-[80vh] flex-col overflow-hidden">
            <div className="flex items-start justify-between gap-3">
              <div>
                <h2 className="text-[15px] font-semibold text-stone-950">
                  {selectedLog.requested_model ?? selectedLog.path}
                </h2>
                <p className="mt-0.5 text-[11px] text-stone-500">
                  {selectedLog.credential_name} · {selectedLog.path}
                </p>
              </div>
              <button
                aria-label="关闭日志详情"
                className="grid h-7 w-7 shrink-0 place-items-center rounded-lg text-stone-400 motion-control hover:bg-stone-100 hover:text-stone-700"
                onClick={() => setSelectedLog(null)}
                type="button"
              >
                <X aria-hidden="true" className="h-4 w-4" />
              </button>
            </div>
            <dl className="mt-3 overflow-y-auto pr-1">
              <LogDetailRow label="结果" value={`${selectedLog.success ? "成功" : "失败"} · HTTP ${selectedLog.status ?? "—"} · ${selectedLog.duration_ms} ms`} />
              <LogDetailRow label="目标" value={selectedLog.target_url ?? "—"} />
              <LogDetailRow label="上游模型" value={selectedLog.upstream_model ?? "—"} />
              <LogDetailRow label="客户端 Key" value={selectedLog.client_key ?? "unknown"} />
              <LogDetailRow label="Trace" value={selectedLog.trace_id ?? "—"} />
              <LogDetailRow label="桥接 / 尝试" value={`${selectedLog.bridge ?? "—"} / ${selectedLog.attempt}`} />
              <LogDetailRow label="时间" value={selectedLog.created_at} />
              {selectedLog.error_message ? (
                <LogDetailRow label="错误" value={selectedLog.error_message} />
              ) : null}
              <LogDetailRow label="客户端请求" value={selectedLog.client_request ?? "—"} />
              <LogDetailRow label="上游请求" value={selectedLog.upstream_request ?? "—"} />
              <LogDetailRow label="上游响应" value={selectedLog.upstream_response ?? "—"} />
              <LogDetailRow label="最终响应" value={selectedLog.final_response ?? "—"} />
              {selectedLog.truncated ? (
                <LogDetailRow
                  label="截断阶段"
                  value={(selectedLog.truncated_stages ?? []).join("、") || "未知"}
                />
              ) : null}
            </dl>
          </div>
        ) : null}
      </MotionDialog>
    </section>
  );
}
