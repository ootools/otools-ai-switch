import { useId, useLayoutEffect, useRef, useState } from "react";
import { ArrowDown, ArrowRight, ArrowUp, ChevronDown, Plus, Trash2 } from "lucide-react";
import { HeaderRulesField, UserAgentField } from "./HeaderRulesField";
import { FailureRulesEditor } from "./FailureRulesEditor";
import { useI18n } from "../lib/i18n";
import type { BuiltinPluginDescriptor, DirectionConfig, PipelineConfig, PluginInstance } from "../lib/api/types";

type Props = {
  pipeline: PipelineConfig;
  catalog: BuiltinPluginDescriptor[];
  onChange: (pipeline: PipelineConfig) => void;
};
type Direction = "request" | "response" | "recovery";
const laneKey = (d: Direction) => d === "recovery" ? "account.pipeline.recoveryLane" : d === "request" ? "account.pipeline.requestLane" : "account.pipeline.responseLane";
type Translate = ReturnType<typeof useI18n>["t"];

function configSummary(item: PluginInstance, descriptor: BuiltinPluginDescriptor | undefined, t: Translate) {
  if (!descriptor) return t("account.pipeline.unavailable");
  const values = descriptor.config_fields.flatMap((field) => {
    const value = item.config[field.key] ?? descriptor.defaults?.[field.key] ?? field.default;
    if (value === undefined || value === null || value === "") return [];
    let label: string;
    if (field.kind === "headers" && Array.isArray(value)) {
      label=`${value.length} 项`;
    } else if (field.kind === "select") {
      label = value === "auto" ? t("account.pipeline.auto") : value === "on" ? t("account.pipeline.on") : value === "off" ? t("account.pipeline.off") : String(value);
    } else if (field.kind === "bool") {
      label = value ? t("account.pipeline.on") : t("account.pipeline.off");
    } else if (typeof value === "number" || typeof value === "string") {
      label = String(value).replace(/\s+/g, " ");
    } else {
      return [];
    }
    return [`${field.label} ${label}`];
  });
  return values.length ? values.join(" · ") : descriptor.config_fields.length ? t("account.pipeline.defaultConfig") : t("account.pipeline.noConfig");
}

function ConfigField({ descriptor, field, value, onChange }: {
  descriptor: BuiltinPluginDescriptor;
  field: BuiltinPluginDescriptor["config_fields"][number];
  value: unknown;
  onChange: (value: unknown) => void;
}) {
  const { t } = useI18n();
  const label = `${descriptor.name} ${field.label}`;
  if (field.kind === "userAgent") return <UserAgentField label={label} value={value} onChange={onChange}/>;
  if (field.kind === "headers") return <HeaderRulesField label={label} value={value} onChange={onChange}/>;
  if (field.kind === "bool") {
    return <label className="pipeline-field pipeline-field-bool">
      <input aria-label={label} checked={Boolean(value)} onChange={(event) => onChange(event.target.checked)} type="checkbox" />
      {field.label}
    </label>;
  }
  if (!["select", "number", "textarea"].includes(field.kind)) return null;
  return <label className={`pipeline-field${field.kind === "textarea" ? " pipeline-field-wide" : ""}`}>
    <span>{field.label}</span>
    {field.kind === "select" ? (
      <select aria-label={label} onChange={(event) => onChange(event.target.value)} value={typeof value === "string" ? value : "auto"}>
        <option value="auto">{t("account.pipeline.auto")}</option>
        <option value="on">{t("account.pipeline.on")}</option>
        <option value="off">{t("account.pipeline.off")}</option>
      </select>
    ) : field.kind === "number" ? (
      <input aria-label={label} min={field.min ?? 0} max={field.max} onChange={(event) => onChange(Number(event.target.value))} type="number" value={typeof value === "number" ? value : 0} />
    ) : (
      <textarea aria-label={label} onChange={(event) => onChange(event.target.value)} rows={3} value={typeof value === "string" ? value : ""} />
    )}
  </label>;
}

function Lane({ direction, config, catalog, onChange }: {
  direction: Direction;
  config: DirectionConfig;
  catalog: BuiltinPluginDescriptor[];
  onChange: (config: DirectionConfig) => void;
}) {
  const { t } = useI18n();
  const id = useId();
  const [adding, setAdding] = useState(false);
  const [search, setSearch] = useState("");
  const [expanded, setExpanded] = useState<number | null>(null);
  const rowButtons = useRef(new Map<number, HTMLButtonElement>());
  const addSelect = useRef<HTMLButtonElement>(null);
  const pendingFocus = useRef<number | "add" | null>(null);
  useLayoutEffect(() => {
    const target = pendingFocus.current;
    if (target === null) return;
    pendingFocus.current = null;
    if (target === "add") addSelect.current?.focus();
    else rowButtons.current.get(target)?.focus();
  }, [config.chain]);
  const passed = config.mode === "passthrough";
  const title = t(laneKey(direction));
  const start = t(direction === "request" ? "account.pipeline.agent" : "account.pipeline.provider");
  const end = t(direction === "request" ? "account.pipeline.provider" : "account.pipeline.agent");
  const available = catalog.filter((entry) =>
    (entry.lane === direction || (entry.lane === "both" && direction !== "recovery")) && !config.chain.some((item) => item.id === entry.id),
  );

  const updateItem = (index: number, update: Partial<PluginInstance>) => {
    onChange({ ...config, chain: config.chain.map((item, i) => i === index ? { ...item, ...update } : item) });
  };
  const addPlugin = (pluginId: string) => {
    if (!available.some((entry) => entry.id === pluginId)) return;
    const index = config.chain.findIndex((item) => item.id === pluginId);
    if (index >= 0) {
      updateItem(index, { enabled: true });
      setExpanded(index);
    } else {
      onChange({ ...config, chain: [...config.chain, { id: pluginId, enabled: true, config: structuredClone(catalog.find(entry => entry.id === pluginId)?.defaults ?? {}) }] });
      setExpanded(config.chain.length);
    }
  };
  const movePlugin = (index: number, delta: number) => {
    const target = index + delta;
    if (target < 0 || target >= config.chain.length || direction === "recovery") return;
    if (catalog.find(e => e.id === config.chain[index].id)?.stage !== catalog.find(e => e.id === config.chain[target].id)?.stage) return;
    const chain = [...config.chain];
    [chain[index], chain[target]] = [chain[target], chain[index]];
    onChange({ ...config, chain });
    setExpanded(target);
    pendingFocus.current = target;
  };
  const removePlugin = (index: number) => {
    onChange({ ...config, chain: config.chain.filter((_, i) => i !== index) });
    setExpanded(null);
    // Restore focus after React mounts the new row order.
    const target = config.chain.length > 1 ? Math.min(index, config.chain.length - 2) : null;
    pendingFocus.current = target ?? "add";
  };

  return <>
    <div className="pipeline-toolbar">
      <div className="pipeline-route">{direction === "recovery" ? <span>失败识别 → 重试 / 续接 / 账号状态</span> : <><span>{start}</span><ArrowRight aria-hidden="true" size={14} /><span>{end}</span></>}</div>
      <div className="pipeline-modes" role="group" aria-label={t("account.pipeline.mode")}>
        {(["passthrough", "process"] as const).map((mode) => (
          <button aria-pressed={config.mode === mode} data-plain-text key={mode} onClick={() => onChange({ ...config, mode })} type="button">
            {t(mode === "passthrough" ? "account.pipeline.passthrough" : "account.pipeline.process")}
          </button>
        ))}
      </div>
    </div>
    {passed ? (
      <div className="pipeline-empty">
        <p>{t("account.pipeline.passthroughHint")}</p>
        <span>{t("account.pipeline.retained")}</span>
      </div>
    ) : <>
      <div className="pipeline-list-heading">
        <span>{direction === "recovery" ? "独立策略 · 展开查看触发条件和参数" : t("account.pipeline.orderHint")}</span>
        <span>{t("account.pipeline.enabledCount", { count: config.chain.filter((item) => item.enabled).length, total: config.chain.length })}</span>
      </div>
      {config.chain.length ? (
        <ol className="pipeline-list" aria-label={t("account.pipeline.pluginList", { direction: title })}>
          {config.chain.map((item, index) => {
            const descriptor = catalog.find((entry) => entry.id === item.id);
            const name = descriptor?.name ?? item.id;
            const open = expanded === index;
            const summary = configSummary(item, descriptor, t);
            const detailsId = `${id}-details-${index}`;
            const buttonId = `${id}-row-${index}`;
            return <li className="pipeline-row" data-expanded={open} key={`${item.id}-${index}`}>
              <div className="pipeline-row-main">
                <button
                  aria-controls={detailsId}
                  aria-expanded={open}
                  aria-label={`${t("account.pipeline.configure")} ${name}`}
                  className="pipeline-row-trigger"
                  data-plain-text
                  id={buttonId}
                  onClick={() => setExpanded(open ? null : index)}
                  ref={(node) => { if (node) rowButtons.current.set(index, node); else rowButtons.current.delete(index); }}
                  type="button"
                >
                  {direction !== "recovery" ? <span aria-hidden="true" className="pipeline-index">{String(index + 1).padStart(2, "0")}</span> : null}
                  <span className="pipeline-row-copy">
                    <span className="pipeline-row-name">{name}</span>
                    <span className="pipeline-row-purpose">{descriptor?.description}</span>
                    <span className="pipeline-row-summary" title={summary}>{summary}</span>
                  </span>
                  <ChevronDown aria-hidden="true" className="pipeline-chevron" size={15} />
                </button>
                <label className="pipeline-toggle">
                  <span>{t(item.enabled ? "account.pipeline.enabled" : "account.pipeline.disabled")}</span>
                  <input
                    aria-label={`${t("account.pipeline.enablePlugin")} ${name}`}
                    checked={item.enabled}
                    onChange={(event) => updateItem(index, { enabled: event.target.checked })}
                    role="switch"
                    type="checkbox"
                  />
                </label>
              </div>
              {open ? (
                <div aria-labelledby={buttonId} className="pipeline-details" id={detailsId} role="region">
                  <p className="pipeline-description">{descriptor ? descriptor.description || t("account.pipeline.noConfig") : t("account.pipeline.unavailable")}</p>
                  {descriptor?.limitations ? <p className="pipeline-description">{descriptor.limitations}</p> : null}
                  {descriptor?.config_fields.length ? (
                    <div className="pipeline-fields">
                      {descriptor.config_fields.map((field) => <ConfigField
                        descriptor={descriptor}
                        field={field}
                        key={field.key}
                        onChange={(value) => updateItem(index, { config: { ...item.config, [field.key]: value } })}
                        value={item.config[field.key] ?? descriptor.defaults?.[field.key] ?? field.default}
                      />)}
                    </div>
                  ) : null}
                  {descriptor?.class_limits_editor ? <details className="pipeline-class-limits"><summary>按错误类别覆盖重试次数</summary><p>留空沿用全局次数，0 表示此类不重试。单次请求共用计数，不会按类别重新累计。</p><div className="pipeline-fields">
                    {Object.entries({transport:"连接错误",server_error:"服务器错误",rate_limit:"限流",capacity:"容量不足",semantic:"其他结构化错误"}).map(([key,label]) => <label className="pipeline-field" key={key}>{label}<input aria-label={`${name} ${label}重试次数`} type="number" min={0} max={1000} placeholder="沿用全局" value={(item.config.retry_class_limits as Record<string,number|null>|undefined)?.[key] ?? ""} onChange={event => updateItem(index,{config:{...item.config,retry_class_limits:{...(item.config.retry_class_limits as object ?? {}),[key]:event.target.value === "" ? null : Number(event.target.value)}}})} /></label>)}
                  </div></details> : null}
                  {Boolean(descriptor?.rule_editor) ? <FailureRulesEditor stream={descriptor?.rule_editor === "stream"} health={descriptor?.rule_editor === "health"} config={item.config} defaults={{...descriptor?.defaults,legacy_rules:descriptor?.legacy_rules}} onChange={config => updateItem(index, { config })} /> : null}
                  <div className="pipeline-actions">
                    {direction !== "recovery" ? <><button aria-label={`${t("account.pipeline.moveUp")} ${name}`} data-plain-text disabled={index === 0 || descriptor?.stage !== catalog.find(e => e.id === config.chain[index - 1]?.id)?.stage} onClick={() => movePlugin(index, -1)} type="button">
                      <ArrowUp aria-hidden="true" size={14} />{t("account.pipeline.moveUp")}
                    </button>
                    <button aria-label={`${t("account.pipeline.moveDown")} ${name}`} data-plain-text disabled={index === config.chain.length - 1 || descriptor?.stage !== catalog.find(e => e.id === config.chain[index + 1]?.id)?.stage} onClick={() => movePlugin(index, 1)} type="button">
                      <ArrowDown aria-hidden="true" size={14} />{t("account.pipeline.moveDown")}
                    </button>
                    </> : null}
                    <button aria-label={`${t("account.pipeline.removePlugin")} ${name}`} className="pipeline-remove" data-plain-text onClick={() => removePlugin(index)} type="button">
                      <Trash2 aria-hidden="true" size={14} />{t("account.pipeline.removePlugin")}
                    </button>
                  </div>
                </div>
              ) : null}
            </li>;
          })}
        </ol>
      ) : <p className="pipeline-empty">{t("account.pipeline.noPlugins")}</p>}
      <button ref={addSelect} className="pipeline-add" type="button" disabled={!available.length && !adding} onClick={() => setAdding(!adding)}><Plus size={16} />{adding ? "关闭中间件目录" : "添加中间件"}</button>
      {adding ? <div className="pipeline-catalog" role="region" aria-label="添加中间件目录">
        <input type="search" aria-label="搜索中间件" placeholder="搜索名称或要解决的问题" value={search} onChange={e => setSearch(e.target.value)} />
        {available.filter(e => `${e.name} ${e.description}`.toLowerCase().includes(search.toLowerCase())).map(entry => <div className="pipeline-catalog-item" key={entry.id}>
          <div><strong>{entry.name}</strong><p>{entry.description}</p></div>
          <button type="button" aria-label={`添加 ${entry.name}`} onClick={() => { addPlugin(entry.id); setAdding(false); setSearch(""); }}>添加</button>
        </div>)}
        {!available.length ? <p>{t("account.pipeline.allAdded")}</p> : null}
      </div> : null}
    </>}
  </>;
}

export function PipelineFlowEditor({ pipeline, catalog, onChange }: Props) {
  const { t } = useI18n();
  const id = useId();
  const [direction, setDirection] = useState<Direction>(pipeline.version >= 2 && !pipeline.request.chain.length && pipeline.recovery?.chain.length ? "recovery" : "request");
  const tabs = useRef(new Map<Direction, HTMLButtonElement>());
  const directions: Direction[] = ["request", "response", "recovery"];
  return <section className="pipeline-settings" aria-label={t("account.pipeline.title")}>
    <div className="pipeline-tabs" role="tablist" aria-label={t("account.pipeline.direction")}>
      {directions.map((lane) => (
        <button
          aria-controls={`${id}-panel-${lane}`}
          aria-label={t(laneKey(lane))}
          aria-selected={direction === lane}
          data-plain-text
          id={`${id}-tab-${lane}`}
          key={lane}
          onClick={() => setDirection(lane)}
          onKeyDown={(event) => {
            let next: Direction;
            if (event.key === "Home") next = "request";
            else if (event.key === "End") next = "recovery";
            else if (event.key === "ArrowLeft" || event.key === "ArrowRight") next = directions[(directions.indexOf(lane) + (event.key === "ArrowLeft" ? 2 : 1)) % 3];
            else return;
            event.preventDefault();
            setDirection(next);
            tabs.current.get(next)?.focus();
          }}
          ref={(node) => { if (node) tabs.current.set(lane, node); else tabs.current.delete(lane); }}
          role="tab"
          tabIndex={direction === lane ? 0 : -1}
          type="button"
        >
          {t(laneKey(lane))}
          <span aria-hidden="true">{pipeline[lane]?.chain.length ?? 0}</span>
        </button>
      ))}
    </div>
    <div aria-labelledby={`${id}-tab-${direction}`} id={`${id}-panel-${direction}`} role="tabpanel" tabIndex={0}>
      <Lane catalog={catalog} config={pipeline[direction] ?? { mode: "process", chain: [] }} direction={direction} key={direction} onChange={(config) => onChange({ ...pipeline, [direction]: config })} />
    </div>
  </section>;
}