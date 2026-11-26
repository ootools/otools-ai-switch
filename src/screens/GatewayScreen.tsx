import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { Copy, List, Play, RefreshCw, ScrollText, Send, Square } from "lucide-react";
import {
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
import { fetchRouteProxyModels } from "../lib/routeProxyModels";

const PLATFORM = "codex";
const cardClass = "space-y-3 rounded-2xl border border-stone-200 bg-white/82 p-4 shadow-sm";
const buttonClass =
  "inline-flex items-center gap-1.5 rounded-xl border border-stone-300 bg-white px-3 py-2 text-[12px] font-semibold text-stone-700 motion-control hover:bg-stone-50 disabled:cursor-not-allowed disabled:opacity-50";
const primaryClass =
  "inline-flex items-center gap-1.5 rounded-xl bg-stone-900 px-3 py-2 text-[12px] font-semibold text-white shadow-sm motion-control hover:bg-stone-800 disabled:cursor-not-allowed disabled:opacity-50";

/** 网关独立页面：启停与端点、模型测试、模型清单、实时日志。 */
export function GatewayScreen() {
  const queryClient = useQueryClient();
  const { t } = useI18n();
  const [model, setModel] = useState("");
  const [toolCall, setToolCall] = useState(false);
  const [outcome, setOutcome] = useState<RoutePoolModelTestOutcome | null>(null);
  const [testError, setTestError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const statusQuery = useQuery({
    queryKey: ["gateway-status"],
    queryFn: getRouteProxyStatus,
    refetchInterval: 5000,
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
  const endpoint = status?.base_url ?? "—";

  const copyEndpoint = async () => {
    if (!status?.base_url) return;
    await copyPlainText(status.base_url);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1400);
  };

  return (
    <section className="space-y-3">
      <div className="rounded-2xl border border-stone-200 bg-white/82 px-4 py-3 shadow-sm">
        <p className="mt-0.5 text-lg font-semibold tracking-tight text-stone-950">网关</p>
        <p className="mt-1 text-[12px] text-stone-500">
          本地 AI 网关的状态、端点、模型测试与运行日志。
        </p>
      </div>

      <div className={cardClass}>
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <h2 className="text-[15px] font-semibold text-stone-950">网关状态</h2>
            <p className="mt-1 text-[12px] text-stone-600">
              {running ? "运行中" : "未运行"} · {status?.route_access_enabled ? "算力池路由已启用" : "算力池路由未启用"}
            </p>
            <p className="mt-1 font-mono text-[12px] text-stone-700" title={endpoint}>
              {endpoint}
            </p>
            {status?.https_base_url ? (
              <p className="mt-0.5 font-mono text-[11px] text-stone-500">{status.https_base_url}</p>
            ) : null}
          </div>
          <div className="flex items-center gap-2">
            <button
              className={buttonClass}
              disabled={!status?.base_url}
              onClick={() => void copyEndpoint()}
              title="复制 HTTP 端点"
              type="button"
            >
              {copied ? <Play aria-hidden="true" className="h-3.5 w-3.5" /> : <Copy aria-hidden="true" className="h-3.5 w-3.5" />}
              {copied ? "已复制" : "复制端点"}
            </button>
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
        {statusQuery.isError ? (
          <p className="text-[12px] text-red-700" role="alert">读取网关状态失败。</p>
        ) : null}
      </div>

      <div className={cardClass}>
        <h2 className="text-[15px] font-semibold text-stone-950">模型测试</h2>
        <p className="text-[12px] text-stone-500">
          向当前算力池发起一次真实生成请求，验证路由是否可用。
        </p>
        <div className="flex flex-wrap items-end gap-2">
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
          <ul className="flex flex-wrap gap-1.5">
            {modelsQuery.data.map((item) => (
              <li
                className="rounded-lg border border-stone-200 bg-white px-2 py-1 font-mono text-[11px] text-stone-700"
                key={item.id}
              >
                {item.id}
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
          <div className="max-h-72 divide-y divide-stone-100 overflow-y-auto rounded-xl border border-stone-200 bg-white">
            {logsQuery.data?.map((entry: RouteProxyLiveLogEntry) => (
              <div className="px-3 py-2 text-[11px]" key={entry.id}>
                <div className="flex flex-wrap items-center gap-2">
                  <span className="font-mono text-stone-800">{entry.requested_model ?? entry.path}</span>
                  <span className={entry.success ? "text-emerald-700" : "text-red-700"}>
                    {entry.status ?? (entry.success ? "成功" : "失败")}
                  </span>
                  <span className="text-stone-500">{entry.duration_ms} ms</span>
                </div>
                <p className="mt-0.5 truncate text-stone-500">
                  {entry.credential_name} · {entry.client_key ?? "unknown"} · {entry.path}
                </p>
                {entry.error_message ? (
                  <p className="mt-0.5 break-all font-mono text-red-700">{entry.error_message}</p>
                ) : null}
              </div>
            ))}
          </div>
        )}
      </div>
    </section>
  );
}
