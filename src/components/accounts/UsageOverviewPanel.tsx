import { keepPreviousData, useQuery, useQueryClient } from "@tanstack/react-query";
import { BarChart3, ChevronLeft, ChevronRight, List, Settings2 } from "lucide-react";
import { useEffect, useState } from "react";
import { ModelPricingDialog } from "./ModelPricingDialog";
import { MotionNumber } from "../motion/MotionPrimitives";
import { UsageTrendChart } from "./UsageTrendChart";
import { exportUsageCsv, getQuotaOverview, getUsageOverview, refreshRouteCredentialsQuota } from "../../lib/api/client";
import { formatCompactCount, formatCostMicros, formatExactCount } from "../../lib/usageFormat";
import { decodeStoredBody, parseUsageMetadata, prettyJsonOrText } from "../../lib/usageMetadata";
import type {
  UsageOverview,
  QuotaOverview,
  UsageOverviewGroupRow,
  UsageOverviewRow,
  UsageRowSource,
} from "../../lib/api/types";

const periods = [
  { key: "today", label: "当日" },
  { key: "24h", label: "24h" },
  { key: "week", label: "本周" },
  { key: "7d", label: "7d" },
  { key: "month", label: "本月" },
  { key: "30d", label: "30d" },
  { key: "all", label: "累计" },
  { key: "custom", label: "自定义" },
] as const;

type Period = (typeof periods)[number]["key"];

const groupDimensions = [
  { key: "model", label: "模型", header: "模型", field: "by_model" },
  { key: "platform", label: "平台", header: "平台", field: "by_platform" },
  { key: "account", label: "账号", header: "账号", field: "by_account" },
  { key: "source", label: "来源", header: "来源", field: "by_source" },
] as const;

type GroupDimension = (typeof groupDimensions)[number]["key"];

/** The grouped numbers as a table, or the same numbers over time as bars. */
const groupViews = [
  { key: "list", label: "列表", Icon: List },
  { key: "chart", label: "图表", Icon: BarChart3 },
] as const;

type GroupView = (typeof groupViews)[number]["key"];

/** The Stats page stays one sidebar item; speed is an in-page view. */
type OverviewView = "usage" | "speed" | "quota";

const pageSize = 20;

/**
 * The transcript scan is the expensive half of this query. A warm scan of a
 * ~1400-file corpus measured 1.1s, so a 10s interval would keep a blocking
 * thread busy over a tenth of the time for numbers that only change when a CLI
 * writes a turn.
 */
const refreshMs = 30_000;

/** Rows shown in a grouping table before the rest are summarized away. */
const groupRowLimit = 12;

function periodSince(period: Period, customSince = "", now = new Date()) {
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  switch (period) {
    case "today": {
      return today.toISOString();
    }
    case "24h": {
      const ms = now.getTime() - 24 * 60 * 60 * 1000;
      return new Date(ms).toISOString();
    }
    case "week": {
      const day = now.getDay();
      const daysFromMonday = day === 0 ? 6 : day - 1;
      const monday = new Date(today);
      monday.setDate(today.getDate() - daysFromMonday);
      return monday.toISOString();
    }
    case "7d": {
      const ms = now.getTime() - 7 * 24 * 60 * 60 * 1000;
      return new Date(ms).toISOString();
    }
    case "month": {
      const firstOfMonth = new Date(now.getFullYear(), now.getMonth(), 1);
      return firstOfMonth.toISOString();
    }
    case "30d": {
      const ms = now.getTime() - 30 * 24 * 60 * 60 * 1000;
      return new Date(ms).toISOString();
    }
    case "all": {
      return null;
    }
    case "custom": {
      return customSince ? new Date(customSince).toISOString() : null;
    }
  }
}

function formatTime(value: string | null | undefined) {
  if (!value) {
    return "-";
  }
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

/** TPS is stored as tok/s × 1000 to stay integer through IPC. */
function formatTps(milli: number | null | undefined) {
  return milli == null ? "—" : `${(milli / 1000).toFixed(1)} tok/s`;
}

const sourceLabels: Record<UsageRowSource, string> = {
  matched: "匹配",
  session_only: "仅会话",
  proxy_only: "仅代理",
};

const sourceStyles: Record<UsageRowSource, string> = {
  matched: "bg-emerald-50 text-emerald-800 ring-1 ring-emerald-200",
  session_only: "bg-blue-50 text-blue-800 ring-1 ring-blue-200",
  proxy_only: "bg-amber-50 text-amber-800 ring-1 ring-amber-200",
};

function errorMessage(error: unknown) {
  if (error instanceof Error && error.message.trim()) {
    return error.message;
  }
  if (error && typeof error === "object") {
    const record = error as { message?: unknown; details?: unknown };
    for (const field of [record.message, record.details]) {
      if (typeof field === "string" && field.trim()) {
        return field.trim();
      }
    }
  }
  return "读取用量失败";
}

function SummaryCard({
  label,
  value,
  title,
}: {
  label: string;
  value: string;
  title: string;
}) {
  return (
    <div className="rounded-xl border border-stone-200 bg-stone-50 p-3">
      <p className="text-[11px] font-medium text-stone-500">{label}</p>
      <p className="mt-1 text-lg font-semibold text-stone-950" title={title}>
        <MotionNumber value={value} />
      </p>
    </div>
  );
}

function GroupTable({
  header,
  rows,
}: {
  header: string;
  rows: UsageOverviewGroupRow[];
}) {
  const shown = rows.slice(0, groupRowLimit);
  const hidden = rows.length - shown.length;
  return (
    <div className="overflow-hidden rounded-xl border border-stone-200">
      <div className="grid grid-cols-[1.6fr_0.6fr_0.8fr_0.8fr_0.8fr_0.8fr] gap-2 border-b border-stone-100 bg-stone-50 px-3 py-2 text-[11px] font-medium text-stone-500">
        <span>{header}</span>
        <span className="text-right">请求</span>
        <span className="text-right">输入</span>
        <span className="text-right">输出</span>
        <span className="text-right">缓存</span>
        <span className="text-right">费用</span>
      </div>
      {shown.length === 0 ? (
        <p className="px-3 py-3 text-[12px] text-stone-500">当前筛选范围内没有数据。</p>
      ) : (
        <div className="divide-y divide-stone-100">
          {shown.map((row) => (
            <div
              className="grid grid-cols-[1.6fr_0.6fr_0.8fr_0.8fr_0.8fr_0.8fr] gap-2 px-3 py-2 text-[12px]"
              key={row.key}
            >
              <span className="truncate text-stone-800" title={row.key}>
                {row.key}
              </span>
              <span className="text-right text-stone-600" title={formatExactCount(row.request_count)}>
                {formatCompactCount(row.request_count)}
              </span>
              <span className="text-right text-stone-600" title={formatExactCount(row.input_tokens)}>
                {formatCompactCount(row.input_tokens)}
              </span>
              <span className="text-right text-stone-600" title={formatExactCount(row.output_tokens)}>
                {formatCompactCount(row.output_tokens)}
              </span>
              <span
                className="text-right text-stone-600"
                title={`缓存读取 ${formatExactCount(row.cache_read_tokens)}；缓存写入 ${formatExactCount(row.cache_write_tokens)}`}
              >
                {formatCompactCount(row.cache_read_tokens + row.cache_write_tokens)}
              </span>
              <span className="text-right text-stone-800">{formatCostMicros(row.cost_micros)}</span>
            </div>
          ))}
        </div>
      )}
      {hidden > 0 ? (
        <p className="border-t border-stone-100 px-3 py-2 text-[11px] text-stone-500">
          另有 {hidden} 项未显示。
        </p>
      ) : null}
    </div>
  );
}

function detailId(rowId: string) {
  return `usage-request-detail-${rowId}`;
}

/** A transcript row has no HTTP status, so it reports the outcome in words. */
function statusText(row: UsageOverviewRow) {
  const outcome = row.success ? "成功" : "失败";
  return row.status ? `${row.status} · ${outcome}` : outcome;
}

function costText(row: UsageOverviewRow) {
  if (!row.price_source) {
    return "无价格";
  }
  const origin = row.price_source === "upstream" ? "上游价格" : "本地价格表估算";
  return `${formatCostMicros(row.cost_micros)}（${origin}）`;
}

function DetailField({
  label,
  value,
  mono,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div>
      <p className="text-[11px] font-medium text-stone-500">{label}</p>
      <p
        className={
          mono
            ? "mt-0.5 break-all font-mono text-[11px] text-stone-700"
            : "mt-0.5 break-all text-stone-800"
        }
      >
        {value}
      </p>
    </div>
  );
}

function DetailBlock({ label, body }: { label: string; body: string }) {
  return (
    <div className="mt-3">
      <p className="text-[11px] font-medium text-stone-500">{label}</p>
      <pre className="mt-1 max-h-56 overflow-auto rounded-lg border border-stone-200 bg-white p-2 font-mono text-[11px] leading-relaxed text-stone-700">
        {body}
      </pre>
    </div>
  );
}

/**
 * Everything known about one request, including the raw proxy metadata.
 *
 * Session-only rows come from a CLI transcript that never touched this proxy, so
 * they have no metadata to show; saying that outright beats an empty panel that
 * reads like a loading failure.
 */
function RequestDetail({ row }: { row: UsageOverviewRow }) {
  const metadata = row.metadata_json ? parseUsageMetadata(row.metadata_json) : null;
  // The preview arrives encoded; decoding is async, so the panel renders without
  // it for the first frame rather than blocking the whole row on a decompress.
  const [responseBody, setResponseBody] = useState<string | null>(null);
  const encodedBody = metadata?.responseBody ?? null;
  useEffect(() => {
    if (!encodedBody) {
      setResponseBody(null);
      return;
    }
    let active = true;
    void decodeStoredBody(encodedBody).then((decoded) => {
      if (active) {
        setResponseBody(decoded);
      }
    });
    return () => {
      active = false;
    };
  }, [encodedBody]);
  const mapping =
    metadata?.requestedModel &&
    metadata.upstreamModel &&
    metadata.requestedModel !== metadata.upstreamModel
      ? `${metadata.requestedModel} → ${metadata.upstreamModel}`
      : null;
  return (
    <div
      aria-label={`请求 ${row.id} 详情`}
      className="border-t border-stone-100 bg-stone-50 px-3 py-3"
      id={detailId(row.id)}
    >
      <div className="flex items-center justify-between gap-2">
        <p className="text-[12px] font-semibold text-stone-800">请求详情</p>
        <p className="break-all font-mono text-[11px] text-stone-500">{row.id}</p>
      </div>
      <div className="mt-3 grid gap-2 text-[12px] sm:grid-cols-2 lg:grid-cols-3">
        <DetailField label="时间" value={formatTime(row.occurred_at)} />
        <DetailField
          label="来源"
          value={
            row.source_label
              ? `${sourceLabels[row.source]} · ${row.source_label}`
              : sourceLabels[row.source]
          }
        />
        <DetailField label="平台" value={row.provider} />
        {/* Which client config sent the request. Rows written before client
            suffixes existed, and internally scoped traffic, have no origin —
            saying unknown beats inventing one from a user agent. */}
        <DetailField label="客户端" value={metadata?.clientKey ?? "unknown"} />
        <DetailField label="模型" value={row.model} />
        {mapping ? <DetailField label="模型映射" value={mapping} /> : null}
        <DetailField label="账号" value={row.account_name ?? "未经代理"} />
        <DetailField label="账号 ID" mono value={row.account_id ?? "-"} />
        <DetailField label="请求路径" mono value={row.path ?? "-"} />
        <DetailField label="状态" value={statusText(row)} />
        <DetailField label="输入 Token" value={formatExactCount(row.input_tokens)} />
        <DetailField label="输出 Token" value={formatExactCount(row.output_tokens)} />
        <DetailField label="缓存写入 Token" value={formatExactCount(row.cache_write_tokens)} />
        <DetailField label="缓存读取 Token" value={formatExactCount(row.cache_read_tokens)} />
        <DetailField label="费用" value={costText(row)} />
        <DetailField label="上游响应 ID" mono value={row.upstream_response_id ?? "-"} />
        {metadata?.durationMs ? (
          <DetailField label="耗时" value={`${metadata.durationMs} ms`} />
        ) : null}
        {metadata?.ttftMs ? (
          <>
            <DetailField label="首 Token 耗时 (TTFT)" value={`${metadata.ttftMs} ms`} />
            <DetailField
              label="输出 TPS"
              value={
                Number(metadata.durationMs) > Number(metadata.ttftMs) && row.output_tokens > 0
                  ? formatTps(
                      Math.floor(
                        (row.output_tokens * 1_000_000) /
                          (Number(metadata.durationMs) - Number(metadata.ttftMs)),
                      ),
                    )
                  : "—"
              }
            />
          </>
        ) : null}
        {metadata?.targetUrl ? (
          <DetailField label="上游地址" mono value={metadata.targetUrl} />
        ) : null}
        {metadata?.traceId ? <DetailField label="追踪 ID" mono value={metadata.traceId} /> : null}
      </div>
      {metadata?.errorMessage ? (
        <DetailBlock body={metadata.errorMessage} label="错误信息" />
      ) : null}
      {responseBody ? (
        <DetailBlock body={prettyJsonOrText(responseBody)} label="上游原始响应" />
      ) : null}
      {metadata ? (
        <DetailBlock
          body={metadata.formatted}
          label={metadata.valid ? "metadata_json" : "metadata_json 无法解析，显示原始内容。"}
        />
      ) : (
        <p className="mt-3 text-[11px] text-stone-500">
          这条记录只来自本机 CLI 会话文件，没有经过本代理，因此没有代理元数据可展示。
        </p>
      )}
    </div>
  );
}

function RequestRow({
  row,
  expanded,
  onToggle,
}: {
  row: UsageOverviewRow;
  expanded: boolean;
  onToggle: () => void;
}) {
  const tokens = row.input_tokens + row.output_tokens;
  const cache = row.cache_write_tokens + row.cache_read_tokens;
  return (
    <div className="bg-white" data-usage-request-row>
      {/* Narrow widths get two tidy lines instead of the seven columns folding into
          a ragged 2-or-3 column block that stranded 详情 mid-row: the timestamp with
          the badges and 详情 pinned to its right, then the labelled fields wrapping
          underneath. `lg:contents` dissolves that second grouping so its fields
          become columns of the wide grid in DOM order, and the `order`/`ml-auto`
          that arrange the narrow lines have to be reset there or they would
          rearrange the grid too. */}
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2.5 text-[12px] text-stone-600 lg:grid lg:grid-cols-[1.3fr_1.5fr_0.8fr_0.8fr_1.1fr_0.8fr_auto] lg:gap-2 lg:items-center">
        <span className="order-1 font-medium text-stone-800 lg:order-none">
          {formatTime(row.occurred_at)}
        </span>
        <div className="order-4 flex w-full min-w-0 flex-wrap items-center gap-x-3 gap-y-1 lg:contents">
          <span className="min-w-0 truncate" title={row.model}>
            <span className="mr-1 text-[10px] text-stone-400 lg:hidden">模型</span>
            {row.model}
          </span>
          <span
            title={`输入 ${formatExactCount(row.input_tokens)}；输出 ${formatExactCount(row.output_tokens)}；缓存写入 ${formatExactCount(row.cache_write_tokens)}；缓存读取 ${formatExactCount(row.cache_read_tokens)}`}
          >
            <span className="mr-1 text-[10px] text-stone-400 lg:hidden">Token</span>
            {formatCompactCount(tokens)}
            {cache > 0 ? <span className="ml-1 text-stone-400">+{formatCompactCount(cache)}</span> : null}
          </span>
          <span title={row.price_source === "estimated" ? "按本地价格表估算" : undefined}>
            <span className="mr-1 text-[10px] text-stone-400 lg:hidden">费用</span>
            {row.price_source ? formatCostMicros(row.cost_micros) : "无价格"}
            {row.price_source === "estimated" ? <span className="text-stone-400">(估)</span> : null}
          </span>
          <span className="min-w-0 truncate" title={row.account_name ?? row.account_id ?? undefined}>
            <span className="mr-1 text-[10px] text-stone-400 lg:hidden">账号</span>
            {row.account_name ?? row.account_id ?? "未经代理"}
          </span>
        </div>
        <span className="order-2 ml-auto flex items-center gap-1.5 lg:order-none lg:ml-0">
          <span
            className={`rounded-md px-1.5 py-0.5 text-[11px] font-semibold ${sourceStyles[row.source]}`}
          >
            {sourceLabels[row.source]}
          </span>
          {!row.success && row.status ? (
            <span className="rounded-md bg-red-50 px-1.5 py-0.5 text-[11px] font-semibold text-red-700 ring-1 ring-red-200">
              {row.status}
            </span>
          ) : null}
        </span>
        <button
          aria-controls={detailId(row.id)}
          aria-expanded={expanded}
          aria-label={`${expanded ? "隐藏" : "查看"}请求 ${row.id} 详情`}
          className="order-3 shrink-0 rounded-lg border border-stone-200 bg-white px-2.5 py-1 text-[12px] font-semibold text-stone-700 motion-control hover:bg-stone-50 lg:order-none lg:justify-self-end"
          onClick={onToggle}
          type="button"
        >
          详情
        </button>
      </div>
      {expanded ? <RequestDetail row={row} /> : null}
    </div>
  );
}

/** Quota tab: read-only snapshot plus renewal warnings from the backend. */
function QuotaView({ data }: { data: QuotaOverview }) {
  const queryClient = useQueryClient();
  const [refreshing, setRefreshing] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  const refresh = async () => {
    setRefreshing(true);
    setMessage(null);
    try {
      const outcomes = await refreshRouteCredentialsQuota("codex");
      await queryClient.invalidateQueries({ queryKey: ["quota-overview"] });
      const failed = outcomes.filter((outcome) => !outcome.updated).length;
      setMessage(
        failed > 0
          ? `已刷新 ${outcomes.length - failed} 个账号，${failed} 个失败。`
          : `已刷新 ${outcomes.length} 个账号。`,
      );
    } catch (error) {
      setMessage(errorMessage(error));
    } finally {
      setRefreshing(false);
    }
  };

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="text-[12px] text-stone-500">
          官方账号额度快照与续期提醒。刷新会向订阅上游查询，不会改写客户端配置。
        </p>
        <button
          className="rounded-lg border border-stone-200 bg-white px-2.5 py-1.5 text-[11px] font-semibold text-stone-600 motion-control hover:bg-stone-50 disabled:opacity-40"
          disabled={refreshing}
          onClick={() => void refresh()}
          type="button"
        >
          {refreshing ? "刷新中…" : "刷新额度"}
        </button>
      </div>
      {message ? <p className="text-[11px] text-stone-500">{message}</p> : null}
      <div className="grid gap-2 sm:grid-cols-3">
        <SummaryCard
          label="官方账号"
          title={formatExactCount(data.official_account_count)}
          value={formatCompactCount(data.official_account_count)}
        />
        <SummaryCard
          label="有额度数据"
          title={formatExactCount(data.with_quota_count)}
          value={formatCompactCount(data.with_quota_count)}
        />
        <SummaryCard
          label="续期提醒"
          title={formatExactCount(data.reminders.length)}
          value={formatCompactCount(data.reminders.length)}
        />
      </div>
      {data.reminders.length > 0 ? (
        <div className="space-y-1 rounded-xl border border-amber-200 bg-amber-50 px-3 py-2 text-[12px] text-amber-900">
          {data.reminders.map((reminder) => (
            <p key={`${reminder.credential_id}-${reminder.window}`}>
              <span className="font-semibold">{reminder.display_name}</span> · {reminder.message}
            </p>
          ))}
        </div>
      ) : null}
      <div className="overflow-hidden rounded-xl border border-stone-200 bg-white">
        <div className="grid grid-cols-[1.4fr_0.8fr_0.8fr_0.8fr_1.2fr] gap-2 border-b border-stone-100 bg-stone-50 px-3 py-2 text-[11px] font-medium text-stone-500">
          <span>账号</span>
          <span className="text-right">5h 剩余</span>
          <span className="text-right">周剩余</span>
          <span className="text-right">限额</span>
          <span className="text-right">最近重置</span>
        </div>
        {data.rows.length === 0 ? (
          <p className="px-3 py-4 text-[12px] text-stone-500">还没有官方订阅账号。</p>
        ) : (
          <div className="divide-y divide-stone-100">
            {data.rows.map((row) => (
              <div
                className="grid grid-cols-[1.4fr_0.8fr_0.8fr_0.8fr_1.2fr] gap-2 px-3 py-2 text-[12px]"
                key={row.id}
              >
                <span className="truncate text-stone-800" title={row.display_name}>
                  {row.display_name}
                  <span className="ml-1 text-[10px] text-stone-400">{row.provider}</span>
                </span>
                <span className="text-right text-stone-600">{formatCompactCount(row.primary_remain ?? 0)}</span>
                <span className="text-right text-stone-600">{formatCompactCount(row.weekly_remain ?? 0)}</span>
                <span className="text-right text-stone-600">
                  {row.quota_limit == null ? "—" : formatCompactCount(row.quota_limit)}
                </span>
                <span className="truncate text-right text-stone-500" title={row.reset_primary ?? undefined}>
                  {formatTime(row.reset_primary)}
                </span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

/**
 * Speed view. Only proxy-backed rows have HTTP timing: a CLI transcript has no
 * network start/first-token instants, so it is excluded rather than counted as
 * a zero. The sample counts beside each number are the honest denominator.
 */
function SpeedView({ overview }: { overview: UsageOverview }) {
  const speed = overview.speed;
  const samples = overview.rows.filter(
    (row) => row.source !== "session_only" && row.ttft_ms != null,
  );
  return (
    <div className="space-y-3">
      <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
        <SummaryCard
          label="TTFT 中位数"
          title={`样本 ${formatExactCount(speed.ttft_sample_count)} 条`}
          value={speed.ttft_p50_ms == null ? "—" : `${speed.ttft_p50_ms} ms`}
        />
        <SummaryCard
          label="TTFT p95"
          title={`样本 ${formatExactCount(speed.ttft_sample_count)} 条`}
          value={speed.ttft_p95_ms == null ? "—" : `${speed.ttft_p95_ms} ms`}
        />
        <SummaryCard
          label="输出 TPS 中位数"
          title={`样本 ${formatExactCount(speed.tps_sample_count)} 条；TPS = 输出 token ÷ (总耗时 − TTFT)`}
          value={formatTps(speed.tps_p50_milli)}
        />
        <SummaryCard
          label="输出 TPS p95"
          title={`样本 ${formatExactCount(speed.tps_sample_count)} 条`}
          value={formatTps(speed.tps_p95_milli)}
        />
      </div>
      <p className="text-[11px] text-stone-400">
        仅统计经过本代理且成功的请求。本机 CLI 会话记录没有网络时间戳，不参与速度统计；流式请求才可能产生首 token 样本，
        非流式与失败请求同样不计入。
      </p>
      <div className="overflow-hidden rounded-xl border border-stone-200 bg-white">
        <div className="grid grid-cols-[1.4fr_1fr_0.8fr_0.8fr_0.8fr] gap-2 border-b border-stone-100 bg-stone-50 px-3 py-2 text-[11px] font-medium text-stone-500">
          <span>模型</span>
          <span>时间</span>
          <span className="text-right">TTFT</span>
          <span className="text-right">输出 TPS</span>
          <span className="text-right">总耗时</span>
        </div>
        {samples.length === 0 ? (
          <p className="px-3 py-4 text-[12px] text-stone-500">当前筛选范围内没有带首 token 计时的请求。</p>
        ) : (
          <div className="divide-y divide-stone-100">
            {samples.slice(0, 20).map((row) => {
              const ttft = row.ttft_ms ?? 0;
              const generation = Math.max(0, (row.duration_ms ?? 0) - ttft);
              return (
                <div
                  className="grid grid-cols-[1.4fr_1fr_0.8fr_0.8fr_0.8fr] gap-2 px-3 py-2 text-[12px]"
                  key={row.id}
                >
                  <span className="truncate text-stone-800" title={row.model}>{row.model}</span>
                  <span className="truncate text-stone-500">{formatTime(row.occurred_at)}</span>
                  <span className="text-right text-stone-600">{ttft} ms</span>
                  <span className="text-right text-stone-600">
                    {generation > 0 && row.output_tokens > 0
                      ? formatTps(Math.floor((row.output_tokens * 1_000_000) / generation))
                      : "—"}
                  </span>
                  <span className="text-right text-stone-600">{row.duration_ms ?? "—"} ms</span>
                </div>
              );
            })}
          </div>
        )}
        {samples.length > 20 ? (
          <p className="border-t border-stone-100 px-3 py-2 text-[11px] text-stone-500">
            另有 {samples.length - 20} 条未显示。
          </p>
        ) : null}
      </div>
    </div>
  );
}

/**
 * `standalone` renders the panel as a top-level page instead of a section
 * hanging under the account workspace's filter bar.
 */
export function UsageOverviewPanel({ standalone = false }: { standalone?: boolean } = {}) {
  const [period, setPeriod] = useState<Period>("today");
  const [page, setPage] = useState(1);
  const [dimension, setDimension] = useState<GroupDimension | null>(null);
  const [groupView, setGroupView] = useState<GroupView>("list");
  const [pricingOpen, setPricingOpen] = useState(false);
  const [expandedRowId, setExpandedRowId] = useState<string | null>(null);
  const [view, setView] = useState<OverviewView>("usage");
  const [customSince, setCustomSince] = useState("");
  const [customUntil, setCustomUntil] = useState("");
  const [exporting, setExporting] = useState(false);
  const [exportNote, setExportNote] = useState<string | null>(null);

  // Custom bounds are explicit and stable, so they belong in the key. Rolling
  // presets derive `since` from `now` and deliberately stay out of it — putting
  // a derived timestamp in the key would refetch on every render.
  const customBounds = period === "custom" ? `${customSince}|${customUntil}` : null;
  const customValid =
    period !== "custom" ||
    (Boolean(customSince) &&
      Boolean(customUntil) &&
      new Date(customSince).getTime() < new Date(customUntil).getTime());
  const query = useQuery({
    queryKey: ["usage-overview", period, page, pageSize, customBounds],
    queryFn: () =>
      getUsageOverview(
        periodSince(period, customSince),
        page,
        pageSize,
        period === "custom" && customUntil ? new Date(customUntil).toISOString() : null,
      ),
    enabled: customValid,
    placeholderData: keepPreviousData,
    refetchInterval: refreshMs,
  });

  const quotaQuery = useQuery({
    queryKey: ["quota-overview"],
    queryFn: getQuotaOverview,
    refetchInterval: refreshMs,
    enabled: view === "quota",
  });

  const overview = query.data;
  const totals = overview?.totals;
  const rowCount = overview?.row_count ?? 0;
  const pageCount = Math.max(1, Math.ceil(rowCount / pageSize));
  const cacheTotal = (totals?.cache_write_tokens ?? 0) + (totals?.cache_read_tokens ?? 0);
  const tokenTotal = (totals?.input_tokens ?? 0) + (totals?.output_tokens ?? 0) + cacheTotal;
  // Every prompt token is exactly one of three things — read from the cache, written
  // into it, or sent fresh — so their sum is the denominator, with no double counting.
  // Cache writes stay in it even though they are misses: a window spent building
  // caches genuinely is a poor hit rate. Output tokens are not part of a prompt and
  // are left out entirely, which would otherwise dilute the ratio with generation.
  const promptTokens =
    (totals?.input_tokens ?? 0) +
    (totals?.cache_write_tokens ?? 0) +
    (totals?.cache_read_tokens ?? 0);
  const cacheHitRate = promptTokens > 0 ? (totals?.cache_read_tokens ?? 0) / promptTokens : null;

  const selectPeriod = (next: Period) => {
    setPeriod(next);
    setPage(1);
    setExportNote(null);
  };

  const handleExport = async () => {
    if (!customValid || exporting) {
      return;
    }
    setExporting(true);
    setExportNote(null);
    try {
      const result = await exportUsageCsv(
        periodSince(period, customSince),
        period === "custom" && customUntil ? new Date(customUntil).toISOString() : null,
      );
      const url = URL.createObjectURL(new Blob([result.content], { type: "text/csv;charset=utf-8" }));
      const link = document.createElement("a");
      link.href = url;
      link.download = `ai-switch-usage-${period}.csv`;
      link.click();
      URL.revokeObjectURL(url);
      setExportNote(
        result.truncated
          ? `已导出前 50000 条（共 ${formatExactCount(result.row_count)} 条，已达上限）。`
          : `已导出 ${formatExactCount(result.row_count)} 条请求。`,
      );
    } catch (error) {
      setExportNote(errorMessage(error));
    } finally {
      setExporting(false);
    }
  };

  const activeGroup = groupDimensions.find((item) => item.key === dimension);
  const groupRows = !overview || !activeGroup ? [] : overview.groups[activeGroup.field];
  const activeSeries = !overview || !activeGroup ? [] : overview.series[activeGroup.field];
  // First-screen trend: one "全部" series summing every dimension row.
  const totalSeries = overview
    ? [{
        key: "全部",
        tokens: overview.series.buckets.map((_, index) =>
          overview.series.by_model.reduce((sum, row) => sum + (row.tokens[index] ?? 0), 0),
        ),
      }]
    : [];
  const chartRows = totalSeries;
  const chartLabel = "全部";

  const integrityNotes = (() => {
    if (!overview) {
      return [];
    }
    const { integrity } = overview;
    const notes = [`已扫描 ${formatExactCount(integrity.scanned_file_count)} 个会话文件`];
    if (integrity.truncated) {
      notes.push("会话文件数量超过扫描上限，以下数字不完整");
    }
    if (integrity.unpriced_request_count > 0) {
      notes.push(
        `其中 ${formatExactCount(integrity.unpriced_request_count)} 个请求的模型没有价格数据，未计入费用`,
      );
    }
    if (integrity.estimated_price_request_count > 0) {
      notes.push(
        `${formatExactCount(integrity.estimated_price_request_count)} 个请求的费用按本地价格表估算`,
      );
    }
    if (integrity.unmatchable_proxy_row_count > 0) {
      notes.push(
        `${formatExactCount(integrity.unmatchable_proxy_row_count)} 条代理记录无法与会话记录匹配，对应请求可能被重复计入`,
      );
    }
    return notes;
  })();

  return (
    <div
      className={`space-y-3 px-3 py-3 ${standalone ? "" : "border-t border-stone-200/80"}`}
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <p className="text-[15px] font-semibold tracking-tight text-stone-950">用量总览</p>
          <p className="mt-0.5 text-[12px] text-stone-500">
            合并本机 CLI 会话记录与代理请求，同一请求只计一次
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <div className="flex gap-1 rounded-xl bg-stone-100 p-1">
            {([["usage", "用量"], ["speed", "速度"], ["quota", "额度"]] as const).map(([key, label]) => (
              <button
                aria-pressed={view === key}
                className={`rounded-lg px-2.5 py-1 text-[11px] font-semibold motion-control ${
                  view === key ? "bg-white text-stone-950 shadow-sm" : "text-stone-500 hover:text-stone-900"
                }`}
                key={key}
                onClick={() => setView(key)}
                type="button"
              >
                {label}
              </button>
            ))}
          </div>
          <button
            aria-label="导出 CSV"
            className="rounded-lg border border-stone-200 bg-white px-2.5 py-1.5 text-[11px] font-semibold text-stone-600 motion-control hover:bg-stone-50 disabled:opacity-40"
            disabled={!customValid || exporting}
            onClick={() => void handleExport()}
            title="导出当前时间窗内的全部请求"
            type="button"
          >
            {exporting ? "导出中…" : "导出 CSV"}
          </button>
          <button
            aria-label="配置模型价格"
            className="grid h-8 w-8 place-items-center rounded-lg border border-stone-200 bg-white text-stone-500 motion-control hover:bg-stone-50 hover:text-stone-900"
            onClick={() => setPricingOpen(true)}
            title="配置模型价格"
            type="button"
          >
            <Settings2 aria-hidden="true" className="h-4 w-4" />
          </button>
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-2 rounded-xl border border-stone-200 bg-white px-3 py-2">
        <span className="shrink-0 text-[11px] font-semibold text-stone-500">时间范围</span>
        <div className="flex min-w-0 flex-1 gap-1 overflow-x-auto">
          {periods.map((item) => (
            <button
              className={`shrink-0 rounded-lg px-2 py-1 text-[11px] font-semibold motion-control ${
                period === item.key
                  ? "bg-stone-900 text-white shadow-sm"
                  : "text-stone-500 hover:bg-stone-100 hover:text-stone-900"
              }`}
              key={item.key}
              onClick={() => selectPeriod(item.key)}
              type="button"
            >
              {item.label}
            </button>
          ))}
        </div>
      </div>

      {period === "custom" ? (
        <div className="flex flex-wrap items-center gap-2 rounded-xl border border-stone-200 bg-white px-3 py-2 text-[12px] text-stone-600">
          <label className="flex items-center gap-2">
            从
            <input
              aria-label="自定义开始时间"
              className="rounded-lg border border-stone-200 bg-white px-2 py-1 text-[12px] outline-none focus:border-blue-400"
              onChange={(event) => setCustomSince(event.target.value)}
              type="datetime-local"
              value={customSince}
            />
          </label>
          <label className="flex items-center gap-2">
            到
            <input
              aria-label="自定义结束时间"
              className="rounded-lg border border-stone-200 bg-white px-2 py-1 text-[12px] outline-none focus:border-blue-400"
              onChange={(event) => setCustomUntil(event.target.value)}
              type="datetime-local"
              value={customUntil}
            />
          </label>
          {!customValid ? <span className="text-[11px] text-red-600">开始时间必须早于结束时间</span> : null}
        </div>
      ) : null}
      {exportNote ? <p className="text-[11px] text-stone-500">{exportNote}</p> : null}

      {view === "quota" ? (
        quotaQuery.isError ? (
          <p className="rounded-lg bg-red-50 px-3 py-2 text-[12px] text-red-700" role="alert">
            {errorMessage(quotaQuery.error)}
          </p>
        ) : !quotaQuery.data ? (
          <p className="text-[12px] text-stone-500" role="status">
            正在读取账号额度…
          </p>
        ) : (
          <QuotaView data={quotaQuery.data} />
        )
      ) : query.isError ? (
        <p className="rounded-lg bg-red-50 px-3 py-2 text-[12px] text-red-700" role="alert">
          {errorMessage(query.error)}
        </p>
      ) : !customValid ? (
        <p className="text-[12px] text-stone-500" role="status">
          请选择有效的开始与结束时间。
        </p>
      ) : !overview ? (
        <p className="text-[12px] text-stone-500" role="status">
          正在读取本机会话记录…首次扫描较慢，之后会走缓存。
        </p>
      ) : view === "speed" ? (
        <SpeedView overview={overview} />
      ) : (
        <>
          <div className="grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
            <SummaryCard
              label="请求"
              title={formatExactCount(totals?.request_count ?? 0)}
              value={formatCompactCount(totals?.request_count ?? 0)}
            />
            <SummaryCard
              label="Token 总计"
              title={`${formatExactCount(tokenTotal)}（输入 + 输出 + 缓存写入 + 缓存读取）`}
              value={formatCompactCount(tokenTotal)}
            />
            <SummaryCard
              label="输入 Token"
              title={formatExactCount(totals?.input_tokens ?? 0)}
              value={formatCompactCount(totals?.input_tokens ?? 0)}
            />
            <SummaryCard
              label="费用（USD）"
              title={`${((totals?.cost_micros ?? 0) / 1_000_000).toFixed(6)} USD`}
              value={formatCostMicros(totals?.cost_micros ?? 0)}
            />
            <SummaryCard
              label="缓存命中率"
              title={
                cacheHitRate === null
                  ? "窗口内没有提示 Token，无从计算命中率"
                  : `缓存读取 ${formatExactCount(totals?.cache_read_tokens ?? 0)} ÷ 提示 ${formatExactCount(promptTokens)}（输入 + 缓存写入 + 缓存读取，不含输出）`
              }
              value={cacheHitRate === null ? "—" : `${(cacheHitRate * 100).toFixed(1)}%`}
            />
          </div>


          <div className="rounded-2xl border border-stone-200 bg-white p-3 shadow-sm">
            <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
              <div>
                <h2 className="text-[14px] font-semibold text-stone-950">Token 趋势</h2>
                <p className="mt-0.5 text-[11px] text-stone-500">
                  当前时间窗的 Token 总量走势
                </p>
              </div>
            </div>
            {overview && !(activeGroup && groupView === "chart") ? (
              <UsageTrendChart
                buckets={overview.series.buckets}
                dimensionLabel={chartLabel}
                rows={chartRows}
                stale={query.isFetching}
                title="Token 趋势总览"
                undatedRequestCount={overview.series.undated_request_count}
                unit={overview.series.unit}
              />
            ) : null}
          </div>

          {integrityNotes.length > 0 ? (
            <p className="text-[11px] text-stone-400">{integrityNotes.join("；")}。</p>
          ) : null}

          <div className="space-y-2">
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-[12px] font-semibold text-stone-600">分组统计</span>
              <div className="flex flex-wrap gap-1 rounded-xl bg-stone-100 p-1">
                {groupDimensions.map((item) => (
                  <button
                    aria-pressed={dimension === item.key}
                    className={`rounded-lg px-2.5 py-1 text-[11px] font-semibold motion-control ${
                      dimension === item.key
                        ? "bg-white text-stone-950 shadow-sm"
                        : "text-stone-500 hover:text-stone-900"
                    }`}
                    key={item.key}
                    onClick={() => setDimension(dimension === item.key ? null : item.key)}
                    type="button"
                  >
                    {item.label}
                  </button>
                ))}
              </div>
              {activeGroup ? (
                <div className="ml-auto flex gap-1 rounded-lg bg-stone-100 p-0.5">
                  {groupViews.map(({ key, label, Icon }) => (
                    <button
                      aria-label={label}
                      aria-pressed={groupView === key}
                      className={`grid h-7 w-7 place-items-center rounded-md motion-control ${
                        groupView === key
                          ? "bg-white text-stone-950 shadow-sm"
                          : "text-stone-500 hover:text-stone-900"
                      }`}
                      key={key}
                      onClick={() => setGroupView(key)}
                      title={label}
                      type="button"
                    >
                      <Icon aria-hidden="true" className="h-3.5 w-3.5" />
                    </button>
                  ))}
                </div>
              ) : null}
            </div>
            {activeGroup && groupView === "list" ? (
              <GroupTable header={activeGroup.header} rows={groupRows} />
            ) : null}
            {activeGroup && groupView === "chart" && overview ? (
              <UsageTrendChart
                buckets={overview.series.buckets}
                dimensionLabel={activeGroup.label}
                rows={activeSeries}
                stale={query.isFetching}
                undatedRequestCount={overview.series.undated_request_count}
                unit={overview.series.unit}
              />
            ) : null}
          </div>

          <div className="overflow-hidden rounded-xl border border-stone-200 bg-white">
            <div className="flex items-center justify-between gap-2 border-b border-stone-100 bg-stone-50 px-3 py-2">
              <p className="text-[12px] font-semibold text-stone-700">请求列表</p>
              <p className="text-[11px] font-medium text-stone-500">
                {formatExactCount(rowCount)} 条
              </p>
            </div>
            {overview.rows.length === 0 ? (
              <p className="px-3 py-4 text-[12px] text-stone-500">当前筛选范围内暂无请求。</p>
            ) : (
              <div className="divide-y divide-stone-100">
                {overview.rows.map((row) => (
                  <RequestRow
                    expanded={expandedRowId === row.id}
                    key={row.id}
                    onToggle={() =>
                      setExpandedRowId((current) => (current === row.id ? null : row.id))
                    }
                    row={row}
                  />
                ))}
              </div>
            )}
            {overview.rows.length > 0 ? (
              <div className="flex items-center justify-between gap-2 border-t border-stone-100 bg-stone-50 px-3 py-2">
                <p className="text-[11px] font-medium text-stone-500">
                  第 {page} / {pageCount} 页
                </p>
                <div className="flex items-center gap-1.5">
                  <button
                    aria-label="上一页"
                    className="grid h-6 w-6 place-items-center rounded-md border border-stone-300 bg-white text-stone-700 motion-control hover:bg-stone-100 disabled:cursor-not-allowed disabled:opacity-40"
                    disabled={page <= 1}
                    onClick={() => setPage((current) => Math.max(1, current - 1))}
                    type="button"
                  >
                    <ChevronLeft aria-hidden="true" className="h-3.5 w-3.5" />
                  </button>
                  <span className="whitespace-nowrap font-mono text-[11px] text-stone-600">
                    {page}/{pageCount}
                  </span>
                  <button
                    aria-label="下一页"
                    className="grid h-6 w-6 place-items-center rounded-md border border-stone-300 bg-white text-stone-700 motion-control hover:bg-stone-100 disabled:cursor-not-allowed disabled:opacity-40"
                    disabled={page >= pageCount}
                    onClick={() => setPage((current) => Math.min(pageCount, current + 1))}
                    type="button"
                  >
                    <ChevronRight aria-hidden="true" className="h-3.5 w-3.5" />
                  </button>
                </div>
              </div>
            ) : null}
          </div>
        </>
      )}
      {pricingOpen ? <ModelPricingDialog open onClose={() => setPricingOpen(false)} /> : null}
    </div>
  );
}
