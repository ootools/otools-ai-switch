import { useQuery } from "@tanstack/react-query";
import { getPrivacyMaskInsights } from "../lib/api/client";

const kindLabels: Record<string, string> = {
  api_key: "密钥 / 令牌",
  email: "邮箱",
  custom_word: "自定义词",
  "model:private_person": "模型：人名",
  "model:private_address": "模型：地址",
  "model:email": "模型：邮箱",
  "model:phone": "模型：电话",
  "model:account": "模型：账号",
  "model:payment_card": "模型：银行卡",
  "model:ip_address": "模型：IP 地址",
  "model:url": "模型：网址",
  "model:common_secret": "模型：常见密钥",
  "model:private_date": "模型：个人日期",
};

function labelFor(kind: string) {
  if (kindLabels[kind]) return kindLabels[kind];
  return kind.startsWith("model:") ? `模型：${kind.slice(6)}` : kind;
}

/** Audit view for local masking. Stores (and shows) placeholders only. */
export function InsightsScreen() {
  const query = useQuery({
    queryKey: ["privacy-mask-insights"],
    queryFn: () => getPrivacyMaskInsights(50),
    refetchInterval: 30_000,
  });
  const rows = query.data ?? [];

  return (
    <section className="space-y-3">
      <div>
        <p className="text-[11px] font-semibold uppercase tracking-wide text-stone-400">Insights</p>
        <h1 className="mt-0.5 text-lg font-semibold text-stone-950">脱敏审计</h1>
        <p className="mt-1 text-[12px] text-stone-500">
          只记录占位符与值的类型、长度、前缀，不保存原文。脱敏失败的请求会被网关拦截并记录在错误日志里。
        </p>
      </div>
      {query.isError ? (
        <p className="rounded-lg bg-red-50 px-3 py-2 text-[12px] text-red-700" role="alert">
          读取脱敏审计失败。
        </p>
      ) : rows.length === 0 ? (
        <p className="rounded-xl border border-stone-200 bg-white px-3 py-4 text-[12px] text-stone-500">
          还没有脱敏记录。开启设置中的「隐私脱敏」后，命中的请求会出现在这里。
        </p>
      ) : (
        <div className="overflow-hidden rounded-xl border border-stone-200 bg-white">
          <div className="grid grid-cols-[1.2fr_0.8fr_0.8fr_1.4fr_0.6fr] gap-2 border-b border-stone-100 bg-stone-50 px-3 py-2 text-[11px] font-medium text-stone-500">
            <span>时间</span>
            <span>类型</span>
            <span>来源</span>
            <span>占位符</span>
            <span className="text-right">长度</span>
          </div>
          <div className="divide-y divide-stone-100">
            {rows.map((row, index) => (
              <div
                className="grid grid-cols-[1.2fr_0.8fr_0.8fr_1.4fr_0.6fr] gap-2 px-3 py-2 text-[12px]"
                key={`${row.occurred_at}:${row.placeholder}:${index}`}
              >
                <span className="truncate text-stone-500">{row.occurred_at}</span>
                <span className="text-stone-700">{labelFor(row.rule_kind)}</span>
                <span className="truncate text-stone-600">{row.platform || "—"}</span>
                <span className="truncate font-mono text-[11px] text-stone-700" title={row.placeholder}>
                  {row.placeholder}
                </span>
                <span className="text-right text-stone-600">{row.char_length}</span>
              </div>
            ))}
          </div>
        </div>
      )}
      <p className="text-[11px] text-stone-400">最近 {rows.length} 条（最多 50 条）。</p>
    </section>
  );
}
