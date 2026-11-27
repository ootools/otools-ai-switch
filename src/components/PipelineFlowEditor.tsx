import { ChevronDown, ChevronUp, Plus, X } from "lucide-react";
import { useI18n } from "../lib/i18n";
import type {
  BuiltinPluginDescriptor,
  DirectionConfig,
  PipelineConfig,
  PipelineMode,
} from "../lib/api/types";
import { useFlowGeometry } from "../lib/useFlowGeometry";

type Props = {
  pipeline: PipelineConfig;
  catalog: BuiltinPluginDescriptor[];
  onChange: (pipeline: PipelineConfig) => void;
};

function ConfigField({
  descriptor,
  field,
  value,
  onChange,
}: {
  descriptor: BuiltinPluginDescriptor;
  field: BuiltinPluginDescriptor["config_fields"][number];
  value: unknown;
  onChange: (value: unknown) => void;
}) {
  const inputClass =
    "mt-1 w-full rounded-md border border-stone-200 bg-white px-2 py-1 text-[11px] text-stone-800 outline-none focus:border-blue-400";
  if (field.kind === "select") {
    return (
      <label className="mt-1.5 block text-[10px] text-stone-500">
        {field.label}
        <select
          aria-label={`${descriptor.name} ${field.label}`}
          className={inputClass}
          onChange={(event) => onChange(event.target.value)}
          value={typeof value === "string" ? value : "auto"}
        >
          <option value="auto">自动</option>
          <option value="on">开启</option>
          <option value="off">关闭</option>
        </select>
      </label>
    );
  }
  if (field.kind === "number") {
    return (
      <label className="mt-1.5 block text-[10px] text-stone-500">
        {field.label}
        <input
          aria-label={`${descriptor.name} ${field.label}`}
          className={inputClass}
          min={0}
          onChange={(event) => onChange(Number(event.target.value))}
          type="number"
          value={typeof value === "number" ? value : 0}
        />
      </label>
    );
  }
  if (field.kind === "textarea") {
    return (
      <label className="mt-1.5 block text-[10px] text-stone-500">
        {field.label}
        <textarea
          aria-label={`${descriptor.name} ${field.label}`}
          className={inputClass}
          onChange={(event) => onChange(event.target.value)}
          rows={2}
          value={typeof value === "string" ? value : ""}
        />
      </label>
    );
  }
  if (field.kind === "bool") {
    return (
      <label className="mt-1.5 flex items-center gap-1.5 text-[10px] text-stone-500">
        <input
          aria-label={`${descriptor.name} ${field.label}`}
          checked={Boolean(value)}
          onChange={(event) => onChange(event.target.checked)}
          type="checkbox"
        />
        {field.label}
      </label>
    );
  }
  return null;
}

function Lane({
  direction,
  title,
  startLabel,
  endLabel,
  config,
  catalog,
  onChange,
}: {
  direction: "request" | "response";
  title: string;
  startLabel: string;
  endLabel: string;
  config: DirectionConfig;
  catalog: BuiltinPluginDescriptor[];
  onChange: (config: DirectionConfig) => void;
}) {
  const { t } = useI18n();
  const passed = config.mode === "passthrough";
  const descriptors = config.chain.map((item) => catalog.find((entry) => entry.id === item.id));
  const nodeIds = passed
    ? [`${direction}-start`, `${direction}-end`]
    : [
        `${direction}-start`,
        ...config.chain.map((_, index) => `${direction}-plugin-${index}`),
        `${direction}-end`,
      ];
  const { boardRef, nodeRefs, paths, size } = useFlowGeometry(nodeIds);

  const setMode = (mode: PipelineMode) => onChange({ ...config, mode });
  const addPlugin = (id: string) => {
    if (!id) return;
    onChange({
      ...config,
      chain: [...config.chain, { id, enabled: true, config: {} }],
    });
  };
  const removePlugin = (index: number) => {
    onChange({ ...config, chain: config.chain.filter((_, itemIndex) => itemIndex !== index) });
  };
  const movePlugin = (index: number, delta: number) => {
    const next = [...config.chain];
    const target = index + delta;
    if (target < 0 || target >= next.length) return;
    [next[index], next[target]] = [next[target], next[index]];
    onChange({ ...config, chain: next });
  };
  const updateConfig = (index: number, key: string, value: unknown) => {
    const next = [...config.chain];
    next[index] = { ...next[index], config: { ...next[index].config, [key]: value } };
    onChange({ ...config, chain: next });
  };

  const available = catalog.filter(
    (entry) => (entry.lane === direction || entry.lane === "both") && !config.chain.some((item) => item.enabled && item.id === entry.id),
  );

  return (
    <section className="rounded-2xl border border-stone-200 bg-white p-3 shadow-sm">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-[13px] font-semibold text-stone-950">{title}</h3>
        <div className="flex gap-1 rounded-xl bg-stone-100 p-1">
          {(["passthrough", "process"] as const).map((mode) => (
            <button
              aria-pressed={config.mode === mode}
              className={`rounded-lg px-2.5 py-1 text-[11px] font-semibold motion-control ${
                config.mode === mode
                  ? "bg-white text-stone-950 shadow-sm"
                  : "text-stone-500 hover:text-stone-900"
              }`}
              key={mode}
              onClick={() => setMode(mode)}
              type="button"
            >
              {t(mode === "passthrough" ? "account.pipeline.passthrough" : "account.pipeline.process")}
            </button>
          ))}
        </div>
      </div>

      {passed ? (
        <p className="mt-2 rounded-lg border border-dashed border-stone-200 bg-stone-50 px-3 py-2 text-[11px] text-stone-500">
          {t("account.pipeline.passthroughHint")}
        </p>
      ) : null}

      <div className="relative mt-3" ref={boardRef}>
        <svg
          aria-hidden="true"
          className="pointer-events-none absolute inset-0"
          height={size.height}
          width={size.width}
        >
          {paths.map((path) => (
            <path
              className="pipeline-flow-line"
              data-direction={direction}
              d={path.d}
              key={path.id}
            />
          ))}
        </svg>
        <div className="relative flex flex-wrap items-stretch gap-2">
          <div
            className="flex min-w-24 flex-col justify-center rounded-xl border border-stone-300 bg-stone-900 px-3 py-2 text-center text-[11px] font-semibold text-white"
            ref={(node) => {
              if (node) nodeRefs.current.set(`${direction}-start`, node);
            }}
          >
            {startLabel}
          </div>

          {!passed
            ? config.chain.map((item, index) => {
                const descriptor = descriptors[index];
                return (
                  <div
                    className="min-w-40 rounded-xl border border-stone-200 bg-stone-50 px-2.5 py-2"
                    key={`${item.id}-${index}`}
                    ref={(node) => {
                      if (node) nodeRefs.current.set(`${direction}-plugin-${index}`, node);
                    }}
                  >
                    <div className="flex items-center gap-1">
                      <span className="min-w-0 flex-1 truncate text-[11px] font-semibold text-stone-800" title={descriptor?.description}>
                        {descriptor?.name ?? item.id}
                      </span>
                      <button
                        aria-label={`${t("account.pipeline.removePlugin")} ${descriptor?.name ?? item.id}`}
                        className="grid h-5 w-5 place-items-center rounded text-stone-400 motion-control hover:bg-red-50 hover:text-red-600"
                        onClick={() => removePlugin(index)}
                        title={t("account.pipeline.removePlugin")}
                        type="button"
                      >
                        <X aria-hidden="true" className="h-3 w-3" />
                      </button>
                    </div>
                    <div className="flex gap-1">
                      <button
                        aria-label="上移"
                        className="grid h-5 w-5 place-items-center rounded text-stone-400 motion-control hover:bg-stone-100 disabled:opacity-30"
                        disabled={index === 0}
                        onClick={() => movePlugin(index, -1)}
                        type="button"
                      >
                        <ChevronUp aria-hidden="true" className="h-3 w-3" />
                      </button>
                      <button
                        aria-label="下移"
                        className="grid h-5 w-5 place-items-center rounded text-stone-400 motion-control hover:bg-stone-100 disabled:opacity-30"
                        disabled={index === config.chain.length - 1}
                        onClick={() => movePlugin(index, 1)}
                        type="button"
                      >
                        <ChevronDown aria-hidden="true" className="h-3 w-3" />
                      </button>
                    </div>
                    {descriptor?.config_fields.map((field) => (
                      <ConfigField
                        descriptor={descriptor}
                        field={field}
                        key={field.key}
                        onChange={(value) => updateConfig(index, field.key, value)}
                        value={item.config[field.key]}
                      />
                    ))}
                  </div>
                );
              })
            : null}

          <div
            className="flex min-w-24 flex-col justify-center rounded-xl border border-stone-300 bg-white px-3 py-2 text-center text-[11px] font-semibold text-stone-800"
            ref={(node) => {
              if (node) nodeRefs.current.set(`${direction}-end`, node);
            }}
          >
            {endLabel}
          </div>
        </div>
      </div>

      {!passed ? (
        <label className="mt-3 flex items-center gap-2 text-[11px] font-semibold text-stone-600">
          <Plus aria-hidden="true" className="h-3.5 w-3.5" />
          {t("account.pipeline.addPlugin")}
          <select
            aria-label={`${t("account.pipeline.addPlugin")} - ${title}`}
            className="ml-auto max-w-52 rounded-lg border border-stone-200 bg-white px-2 py-1 text-[11px] outline-none focus:border-blue-400"
            onChange={(event) => {
              addPlugin(event.target.value);
              event.target.value = "";
            }}
            value=""
          >
            <option value="">—</option>
            {available.map((entry) => (
              <option key={entry.id} value={entry.id}>
                {entry.name}
              </option>
            ))}
          </select>
        </label>
      ) : null}
    </section>
  );
}

export function PipelineFlowEditor({ pipeline, catalog, onChange }: Props) {
  const { t } = useI18n();
  return (
    <div className="space-y-3">
      <Lane
        catalog={catalog}
        config={pipeline.request}
        direction="request"
        endLabel={t("account.pipeline.provider")}
        onChange={(config) => onChange({ ...pipeline, request: config })}
        startLabel={t("account.pipeline.agent")}
        title={t("account.pipeline.requestLane")}
      />
      <Lane
        catalog={catalog}
        config={pipeline.response}
        direction="response"
        endLabel={t("account.pipeline.agent")}
        onChange={(config) => onChange({ ...pipeline, response: config })}
        startLabel={t("account.pipeline.provider")}
        title={t("account.pipeline.responseLane")}
      />
    </div>
  );
}
