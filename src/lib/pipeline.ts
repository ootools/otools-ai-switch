import builtinManifest from "../../native/src/services/builtin_plugins/scripts/manifest.json";
import defaultRules from "../../native/src/services/builtin_plugins/scripts/failure-rules.json";
import type { PipelineConfig, RouteCredentialFailurePolicy } from "./api/types";
import {
  plaintextReasoningMatchedHost,
} from "./responsesPlaintextReasoning";
import {
  crossResourceStripMatchedHost,
} from "./responsesCrossResourceStrip";
import {
  requestCompressionMatchedHost,
} from "./requestCompression";

export const REQUEST_TRANSFORM_PLUGINS = [
  "builtin.request.compression",
  "builtin.request.contentRejection",
  "builtin.request.imageLimit",
  "builtin.request.forceReasoning",
  "builtin.request.inlineRemoteImages",
  "builtin.request.turnReminder",
  "builtin.request.customToolCompat",
] as const;

export const REQUEST_SCHEDULE_PLUGINS = [
  "builtin.schedule.retry",
  "builtin.schedule.semanticThreshold",
  "builtin.schedule.cooldown",
  "builtin.schedule.errorStatus",
  "builtin.schedule.streamPrelude",
  "builtin.schedule.streamSalvage",
  "builtin.schedule.streamContinue",
  "builtin.schedule.failover",
] as const;

export const RESPONSE_PLUGINS = [
  "builtin.response.plaintextReasoning",
  "builtin.response.crossResourceStrip",
  "builtin.response.encryptedContentCleanup",
] as const;

const emptyLimits = {
  transport: null,
  server_error: null,
  rate_limit: null,
  capacity: null,
  semantic: null,
  other: null,
};

export function defaultFailurePolicy(): RouteCredentialFailurePolicy {
  return {
    retry_count: 2,
    retry_interval_ms: 200,
    semantic_error_threshold: 10,
    cooldown_enabled: false,
    cooldown_seconds: 10,
    error_status_enabled: true,
    stream_continue_enabled: false,
    stream_continue_max: 10,
    failover_enabled: true,
    retry_class_limits: emptyLimits,
    capacity_retry_interval_ms: 0,
    max_retry_elapsed_seconds: 0,
    stream_prelude_wait_ms: 60000,
    stream_salvage_enabled: false,
  } as RouteCredentialFailurePolicy;
}

function plug(
  id: string,
  enabled: boolean,
  config: Record<string, unknown> = {},
): { id: string; enabled: boolean; config: Record<string, unknown> } {
  return { id, enabled, config };
}

/** New accounts only install useful defaults; absent plugins stay absent. */
export function createDefaultPipeline(baseUrl = ""): PipelineConfig {
  const request = [];
  if (requestCompressionMatchedHost(baseUrl)) request.push(plug("builtin.request.compression", true, { mode: "auto" }));
  if (plaintextReasoningMatchedHost(baseUrl)) request.push(plug("builtin.request.plaintextReasoning", true, { mode: "auto" }));
  if (crossResourceStripMatchedHost(baseUrl)) request.push(plug("builtin.request.crossResourceStrip", true, { mode: "auto" }));
  return { version: 2, catalog_revision: 1, request: { mode: "process", chain: request }, response: { mode: "process", chain: [] }, recovery: { mode: "process", chain: recoveryPlugins(defaultFailurePolicy()) } };
}

function recoveryPlugins(policy: RouteCredentialFailurePolicy, legacy = false) {
  const { retry_count, retry_interval_ms, max_retry_elapsed_seconds, capacity_retry_interval_ms, retry_class_limits, failover_enabled, stream_prelude_wait_ms,
    cooldown_enabled, cooldown_seconds, semantic_error_threshold, error_status_enabled, stream_continue_enabled, stream_continue_max, stream_salvage_enabled } = policy;
  const plugins = [
    plug("builtin.recovery.retry", retry_count > 0 || failover_enabled || stream_prelude_wait_ms > 0, { retry_count, retry_interval_ms, max_retry_elapsed_seconds, capacity_retry_interval_ms, retry_class_limits, failover_enabled, stream_prelude_wait_ms, ...(legacy ? { legacy_matching: true } : { rules: structuredClone(defaultRules) }) }),
    plug("builtin.recovery.accountHealth", cooldown_enabled || error_status_enabled, { cooldown_enabled, cooldown_seconds, semantic_error_threshold, error_status_enabled }),
  ];
  if (stream_continue_enabled || stream_salvage_enabled) plugins.push(plug("builtin.recovery.stream", true, { stream_continue_enabled, stream_continue_max, stream_salvage_enabled, retry_count, retry_interval_ms, max_retry_elapsed_seconds, capacity_retry_interval_ms, retry_class_limits, failover_enabled }));
  return plugins;
}

/** V1's disabled schedule rows did not zero native defaults. Preserve actual behavior. */
export function migratePipeline(raw: PipelineConfig, base = defaultFailurePolicy()): PipelineConfig {
  if (raw.version >= 2) return { ...raw, recovery: raw.recovery ?? { mode: "process", chain: [] } };
  const policy = failurePolicyFromPipeline(raw, base);
  const oldSchedule = raw.request.chain.filter(p => p.id.startsWith("builtin.schedule."));
  const request = raw.request.chain.filter(p => !p.id.startsWith("builtin.schedule.")).map(p => raw.request.mode === "passthrough" ? { ...p, enabled: false } : p);
  let requestMode = raw.request.mode;
  const response = [];
  for (const p of raw.response.chain) {
    if ((RESPONSE_PLUGINS as readonly string[]).includes(p.id)) {
      if (p.enabled && raw.response.mode !== "passthrough") requestMode = "process";
      request.push({ ...p, id: p.id.replace("builtin.response.", "builtin.request."), enabled: p.enabled && raw.response.mode !== "passthrough" });
    } else response.push(p);
  }
  const recovery = recoveryPlugins(policy, true);
  for (const plugin of recovery) plugin.config.legacy_plugins = oldSchedule;
  const retained = request.filter(p => p.enabled || p.id.startsWith("pack:") || !isUntouchedDisabled(p));
  return { version: 2, request: { ...raw.request, mode: requestMode, chain: retained }, response: { ...raw.response, chain: response }, recovery: { mode: raw.request.mode, chain: recovery } };
}

function isUntouchedDisabled(p: { id: string; config: Record<string, unknown> }) {
  const empty = Object.keys(p.config).length === 0;
  if (!(REQUEST_TRANSFORM_PLUGINS as readonly string[]).includes(p.id) && !["builtin.request.plaintextReasoning", "builtin.request.crossResourceStrip", "builtin.request.encryptedContentCleanup"].includes(p.id)) return false;
  return empty || JSON.stringify(p.config) === '{"mode":"off"}' || JSON.stringify(p.config) === '{"text":""}' || JSON.stringify(p.config) === '{"aggressive_strip":false}';
}

function upgradeCatalog(pipeline: PipelineConfig, config: Record<string, unknown>): PipelineConfig {
  if ((pipeline.catalog_revision ?? 0) >= 1) return pipeline;
  const chain: typeof pipeline.request.chain = [];
  let merged: typeof chain[number] | undefined;
  const hasImages = pipeline.request.chain.some(p => p.id === "builtin.request.imageProcessing");
  const redundant: typeof chain = [];
  let requestMode=pipeline.request.mode;
  for (const plugin of pipeline.request.chain) {
    const key = plugin.id === "builtin.request.inlineRemoteImages" ? "inline_remote" : plugin.id === "builtin.request.imageLimit" ? "trim_old_images" : null;
    if (!key) { chain.push(plugin); continue; }
    if (hasImages) { redundant.push(plugin); continue; }
    if (!merged) { merged = plug("builtin.request.imageProcessing", false, { inline_remote:false, trim_old_images:false, max_images:10, legacy_plugins:[] }); chain.push(merged); }
    merged.enabled ||= plugin.enabled; merged.config[key] = plugin.enabled;
    (merged.config.legacy_plugins as unknown[]).push(plugin);
  }
  if (redundant.length) { const index=chain.findIndex(p=>p.id === "builtin.request.imageProcessing"); const item=chain[index]; chain[index]={...item,config:{...item.config,legacy_plugins:[...(Array.isArray(item.config.legacy_plugins)?item.config.legacy_plugins:[]),...redundant]}}; }
  if (!chain.some(p => p.id === "builtin.request.requestHeaders") && config.headers && typeof config.headers === "object" && !Array.isArray(config.headers)) {
    let user_agent = ""; const headers = [];
    for (const [name, value] of Object.entries(config.headers)) {
      if (typeof value !== "string" || !value.trim()) continue;
      if (name.toLowerCase() === "user-agent") user_agent = value.trim();
      else headers.push({ name, value:value.trim(), operation:"fill" });
    }
    if (user_agent || headers.length) {
      if (requestMode === "passthrough") { for (let i=0;i<chain.length;i++) chain[i]={...chain[i],enabled:false}; requestMode="process"; }
      chain.push(plug("builtin.request.requestHeaders", true, { user_agent, headers }));
    }
  }
  return {...pipeline, catalog_revision:1, request:{...pipeline.request,mode:requestMode,chain}};
}

export function resolvePipeline(configJson: string, baseUrl = ""): PipelineConfig {
  try {
    const config = JSON.parse(configJson);
    const raw = config.pipeline as PipelineConfig | undefined;
    if (raw && [1, 2].includes(raw.version)) {
      const normalized:PipelineConfig={...raw,request:raw.request ?? {mode:"process",chain:[]},response:raw.response ?? {mode:"process",chain:[]},recovery:raw.recovery ?? {mode:"process",chain:[]}};
      return upgradeCatalog(migratePipeline(normalized, { ...defaultFailurePolicy(), ...config.failure_policy }),config);
    }
    const pipeline = createDefaultPipeline(baseUrl);
    if (config.failure_policy) pipeline.recovery = { mode: "process", chain: recoveryPlugins({ ...defaultFailurePolicy(), ...config.failure_policy }, true) };
    const mappings: [string, string, string, unknown][] = [
      ["request_brotli_compression", "compression", "mode", "off"], ["responses_plaintext_reasoning_compat", "plaintextReasoning", "mode", "off"], ["responses_cross_resource_strip", "crossResourceStrip", "mode", "off"],
      ["responses_encrypted_content_cleanup", "encryptedContentCleanup", "aggressive_strip", false], ["inline_remote_images", "inlineRemoteImages", "", false], ["force_reasoning_content", "forceReasoning", "", false],
      ["image_limit_compat", "imageLimit", "", "off"], ["responses_custom_tool_compat", "customToolCompat", "", false], ["content_rejection_compat", "contentRejection", "", "off"],
    ];
    for (const [key, suffix, field, off] of mappings) {
      if (config[key] === undefined) continue;
      const id = `builtin.request.${suffix}`;
      pipeline.request.chain = pipeline.request.chain.filter(p => p.id !== id);
      if (config[key] !== off && config[key] !== false) pipeline.request.chain.push(plug(id, true, field ? { [field]: suffix === "encryptedContentCleanup" ? config.responses_encrypted_content_aggressive_strip === true : config[key] } : {}));
    }
    if (config.turn_reminder === true) pipeline.request.chain.push(plug("builtin.request.turnReminder",true,{text:config.turn_reminder_text ?? ""}));
    return upgradeCatalog({...pipeline,catalog_revision:0},config);
  } catch { return createDefaultPipeline(baseUrl); }
}

function num(config: Record<string, unknown>, key: string, fallback: number): number {
  return typeof config[key] === "number" ? (config[key] as number) : fallback;
}

export function failurePolicyFromPipeline(
  pipeline: PipelineConfig,
  base: RouteCredentialFailurePolicy,
): RouteCredentialFailurePolicy {
  if (pipeline.version >= 2) {
    const zero = failurePolicyFromPipeline({ version: 1, request: { mode: "passthrough", chain: [] }, response: { mode: "process", chain: [] } }, base);
    if (pipeline.recovery?.mode === "passthrough") return zero;
    for (const item of pipeline.recovery?.chain ?? []) {
      if (!item.enabled) continue;
      const descriptor=builtinManifest.find(p=>p.id===item.id);
      const keys = descriptor?.policy_domain === "retry" ? ["retry_count", "retry_interval_ms", "max_retry_elapsed_seconds", "capacity_retry_interval_ms", "retry_class_limits", "failover_enabled", "stream_prelude_wait_ms"]
        : descriptor?.policy_domain === "health" ? ["cooldown_enabled", "cooldown_seconds", "semantic_error_threshold", "error_status_enabled"]
        : descriptor?.policy_domain === "stream" ? ["stream_continue_enabled", "stream_continue_max", "stream_salvage_enabled"] : [];
      const config = { ...builtinManifest.find(p => p.id === item.id)?.defaults, ...item.config };
      for (const key of keys) if ((config as Record<string, unknown>)[key] !== undefined) Object.assign(zero, { [key]: (config as Record<string, unknown>)[key] });
    }
    return zero;
  }
  const policy: RouteCredentialFailurePolicy = { ...base, retry_class_limits: { ...base.retry_class_limits } };
  if (pipeline.request.mode === "passthrough") {
    return {
      ...policy,
      retry_count: 0,
      retry_interval_ms: 0,
      semantic_error_threshold: 0,
      cooldown_enabled: false,
      error_status_enabled: false,
      stream_continue_enabled: false,
      stream_salvage_enabled: false,
      failover_enabled: false,
      stream_prelude_wait_ms: 0,
      capacity_retry_interval_ms: 0,
      max_retry_elapsed_seconds: 0,
      retry_class_limits: emptyLimits,
    };
  }
  for (const plugin of pipeline.request.chain) {
    if (!plugin.enabled) continue;
    const cfg = plugin.config;
    switch (plugin.id) {
      case "builtin.schedule.retry":
        policy.retry_count = num(cfg, "retry_count", policy.retry_count);
        policy.retry_interval_ms = num(cfg, "retry_interval_ms", policy.retry_interval_ms);
        policy.max_retry_elapsed_seconds = num(cfg, "max_retry_elapsed_seconds", policy.max_retry_elapsed_seconds);
        policy.capacity_retry_interval_ms = num(cfg, "capacity_retry_interval_ms", policy.capacity_retry_interval_ms);
        if (cfg.retry_class_limits && typeof cfg.retry_class_limits === "object") {
          policy.retry_class_limits = cfg.retry_class_limits as RouteCredentialFailurePolicy["retry_class_limits"];
        }
        break;
      case "builtin.schedule.semanticThreshold":
        policy.semantic_error_threshold = num(cfg, "value", policy.semantic_error_threshold);
        break;
      case "builtin.schedule.cooldown":
        policy.cooldown_enabled = true;
        policy.cooldown_seconds = num(cfg, "seconds", policy.cooldown_seconds);
        break;
      case "builtin.schedule.errorStatus":
        policy.error_status_enabled = true;
        break;
      case "builtin.schedule.streamPrelude":
        policy.stream_prelude_wait_ms = num(cfg, "ms", policy.stream_prelude_wait_ms);
        break;
      case "builtin.schedule.streamSalvage":
        policy.stream_salvage_enabled = true;
        break;
      case "builtin.schedule.streamContinue":
        policy.stream_continue_enabled = true;
        policy.stream_continue_max = num(cfg, "max", policy.stream_continue_max);
        break;
      case "builtin.schedule.failover":
        policy.failover_enabled = true;
        break;
      default:
        break;
    }
  }
  return policy;
}

/** Only the pipeline is persisted; flat legacy keys are never written. */
export function writePipelineToConfig(
  config: Record<string, unknown>,
  pipeline: PipelineConfig,
): void {
  config.pipeline = pipeline;
  if ((pipeline.catalog_revision ?? 0) >= 1) delete config.headers;
}
