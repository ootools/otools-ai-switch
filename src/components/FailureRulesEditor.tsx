import { useState } from "react";
import { getTransport } from "../lib/transport";

export type FailureCondition = { field: string; op: string; value: string };
export type FailureRule = { id: string; name: string; enabled: boolean; groups: FailureCondition[][]; exclude: FailureCondition[]; action: string; class: string };
type Props = { config: Record<string, unknown>; defaults: Record<string, unknown>; health?: boolean; stream?: boolean; onChange: (config: Record<string, unknown>) => void };
const fields = { status: "HTTP 状态", code: "error.code", type: "error.type", code_or_type: "code 或 type", message: "错误消息", transport: "连接错误" };
const ops = { equals: "等于", contains: "包含", equals_any: "等于任一项", contains_any: "包含任一项", starts_with: "开头是", range: "范围", present: "非空" };
function Condition({ value, onChange, onRemove }: { value: FailureCondition; onChange: (v: FailureCondition) => void; onRemove: () => void }) {
  return <div className="failure-condition">
    <select aria-label="匹配字段" value={value.field} onChange={e => onChange({ ...value, field: e.target.value })}>{Object.entries(fields).map(([v, label]) => <option key={v} value={v}>{label}</option>)}</select>
    <select aria-label="匹配方式" value={value.op} onChange={e => onChange({ ...value, op: e.target.value })}>{Object.entries(ops).map(([v, label]) => <option key={v} value={v}>{label}</option>)}</select>
    {value.op.endsWith("_any") ? <textarea aria-label="匹配内容" rows={Math.min(8, Math.max(3,value.value.split("\n").length))} placeholder="每行一个值，任一项匹配即可" value={value.value} onChange={e => onChange({ ...value, value: e.target.value })} /> : <input aria-label="匹配内容" value={value.value} onChange={e => onChange({ ...value, value: e.target.value })} />}
    <button type="button" onClick={onRemove} aria-label="删除条件">×</button>
  </div>;
}
const blank = (): FailureCondition => ({ field: "code", op: "equals", value: "" });
export function FailureRulesEditor({ config, defaults, health = false, stream = false, onChange }: Props) {
  const rules = (Array.isArray(config.rules) ? config.rules : config.legacy_matching === true && Array.isArray(defaults.legacy_rules) ? defaults.legacy_rules : Array.isArray(defaults.rules) ? defaults.rules : []) as FailureRule[];
  const [sample, setSample] = useState('{"error":{"code":"no_capacity","message":"high demand"}}');
  const [status, setStatus] = useState("200");
  const [transport, setTransport] = useState(false);
  const [result, setResult] = useState("");
  const [busy, setBusy] = useState(false);
  const save = (next: FailureRule[]) => onChange({ ...config, legacy_matching: false, rules: next });
  const update = (i: number, patch: Partial<FailureRule>) => save(rules.map((r, n) => n === i ? { ...r, ...patch } : r));
  return <div className="failure-rules">
    <div className="failure-rules-heading"><strong>错误匹配规则</strong><button type="button" onClick={() => save(structuredClone((defaults.rules ?? []) as FailureRule[]))}>恢复默认规则</button></div>
    <p>按顺序使用第一条命中的规则。同组条件同时满足，任一组满足即可；排除条件优先。字符串忽略大小写及多余空白。</p>
    {config.legacy_matching === true && !config.rules ? <p role="note">当前保留旧配置的宽泛重试（含其他结构化错误和非成功状态），下列兼容规则展示其范围。编辑或恢复默认规则后切换到明确匹配；测试用于预览切换后的规则结果。</p> : null}
    <p>{health ? "仅对代理已判定的账号故障进一步筛选；未匹配或选择忽略时不触发冷却/异常标记。凭据撤销仍由认证机制识别。" : "401/403、明确额度耗尽、权限不足、上下文超限等错误不能通过原样重试解决。正常正文不参与错误消息匹配。"}</p>
    {rules.map((rule, i) => <details className="failure-rule" key={rule.id}>
      <summary>{rule.enabled ? "●" : "○"} {rule.name} · {rule.groups.length} 组条件 · {rule.action === "retry" ? "重试" : rule.action === "next_account" ? "换账号" : rule.action === "record" ? "参与故障处理" : rule.action === "ignore" ? "不触发冷却或异常标记" : "返回错误"}</summary>
      <label><input type="checkbox" checked={rule.enabled} onChange={e => update(i, { enabled: e.target.checked })} />启用此规则</label>
      <label>规则名称<input aria-label="规则名称" value={rule.name} onChange={e => update(i, { name: e.target.value })} /></label>
      {rule.groups.map((group, g) => <fieldset key={g}><legend>条件组 {g + 1} · 同时满足</legend>
        {group.map((c, n) => <Condition key={n} value={c} onChange={v => update(i, { groups: rule.groups.map((x, j) => j === g ? x.map((y, k) => k === n ? v : y) : x) })} onRemove={() => update(i, { groups: rule.groups.map((x, j) => j === g ? x.filter((_, k) => k !== n) : x).filter(x => x.length) })} />)}
        <button type="button" onClick={() => update(i, { groups: rule.groups.map((x, j) => j === g ? [...x, blank()] : x) })}>添加同时满足条件</button>
      </fieldset>)}
      <button type="button" onClick={() => update(i, { groups: [...rule.groups, [blank()]] })}>添加另一组条件（或）</button>
      <fieldset><legend>排除条件 · 任一匹配则跳过</legend>{(rule.exclude ?? []).map((c, n) => <Condition key={n} value={c} onChange={v => update(i, { exclude: rule.exclude.map((x, j) => j === n ? v : x) })} onRemove={() => update(i, { exclude: rule.exclude.filter((_, j) => j !== n) })} />)}<button type="button" onClick={() => update(i, { exclude: [...(rule.exclude ?? []), blank()] })}>添加排除条件</button></fieldset>
      <label>命中动作<select aria-label="命中动作" value={rule.action} onChange={e => update(i, { action: e.target.value })}>{health ? <><option value="record">参与故障处理</option><option value="ignore">不触发冷却或异常标记</option></> : <><option value="retry">重试（受次数和时限约束）</option>{!stream ? <option value="next_account">转下个账号（需允许换号）</option> : null}<option value="return">返回错误</option></>}</select></label>
      {!health ? <label>错误类别<select aria-label="错误类别" value={rule.class} onChange={e => update(i, { class: e.target.value })}>{Object.entries({ capacity: "上游容量不足（不惩罚账号）", rate_limit: "限流", server_error: "服务器错误", transport: "连接故障", semantic: "其他结构化错误", other: "其他错误" }).map(([k, v]) => <option key={k} value={k}>{v}</option>)}</select></label> : null}
      <div className="pipeline-actions"><button type="button" disabled={i === 0} onClick={() => { const next = [...rules]; [next[i - 1], next[i]] = [next[i], next[i - 1]]; save(next); }}>提前匹配</button><button type="button" onClick={() => save(rules.filter((_, j) => i !== j))}>删除规则</button></div>
    </details>)}
    <button type="button" onClick={() => save([...rules, { id: crypto.randomUUID(), name: "自定义错误", enabled: true, groups: [[blank()]], exclude: [], action: health ? "record" : "retry", class: "semantic" }])}>添加错误规则</button>
    <details><summary>测试错误样例</summary>
      <label>HTTP 状态<input aria-label="样例 HTTP 状态" type="number" min={100} max={599} value={status} onChange={e => setStatus(e.target.value)} /></label>
      <label><input type="checkbox" checked={transport} onChange={e => setTransport(e.target.checked)} />连接或读取失败</label>
      <label>JSON / SSE 错误<textarea aria-label="错误样例" rows={4} value={sample} onChange={e => setSample(e.target.value)} /></label>
      <button type="button" disabled={busy} onClick={async () => { setBusy(true); try { const r = await getTransport().call<{ rule_name: string; action: string; observed: unknown }>("builtin_plugin_test_rules", { config: { ...config, rules }, status: status ? Number(status) : null, body: sample, transport, health }); setResult(`${r.rule_name} → ${r.action === "retry" ? "重试" : r.action === "next_account" ? "换账号" : r.action === "record" ? "参与故障处理" : r.action === "ignore" ? "不触发冷却或异常标记" : "返回错误"}\n${JSON.stringify(r.observed, null, 2)}`); } catch (e) { setResult(`测试失败：${e instanceof Error ? e.message : String(e)}`); } finally { setBusy(false); } }}>测试匹配</button>
      <pre aria-live="polite">{result}</pre>
    </details>
  </div>;
}
