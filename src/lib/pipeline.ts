import type { PipelineConfig, RouteCredentialFailurePolicy } from "./api/types";
import {
  plaintextReasoningMatchedHost,
  type ResponsesPlaintextReasoningMode,
} from "./responsesPlaintextReasoning";
import {
  crossResourceStripMatchedHost,
  type ResponsesCrossResourceStripMode,
} from "./responsesCrossResourceStrip";
import {
  requestCompressionMatchedHost,
  type RequestCompressionMode,
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

/**
 * Built-ins are off by default. Only plugins whose declared host list matches
 * the provider's base URL turn themselves on automatically.
 */
export function createDefaultPipeline(baseUrl = ""): PipelineConfig {
  const compressionMode: RequestCompressionMode = requestCompressionMatchedHost(baseUrl) ? "auto" : "off";
  const plaintextMode: ResponsesPlaintextReasoningMode = plaintextReasoningMatchedHost(baseUrl) ? "auto" : "off";
  const stripMode: ResponsesCrossResourceStripMode = crossResourceStripMatchedHost(baseUrl) ? "auto" : "off";
  return {
    version: 1,
    request: {
      mode: "process",
      chain: [
        plug("builtin.request.compression", compressionMode === "auto", { mode: compressionMode }),
        plug("builtin.request.contentRejection", false),
        plug("builtin.request.imageLimit", false),
        plug("builtin.request.forceReasoning", false),
        plug("builtin.request.inlineRemoteImages", false),
        plug("builtin.request.turnReminder", false, { text: "" }),
        plug("builtin.request.customToolCompat", false),
        plug("builtin.schedule.retry", false, { retry_count: 2, retry_interval_ms: 200, max_retry_elapsed_seconds: 0, capacity_retry_interval_ms: 0 }),
        plug("builtin.schedule.semanticThreshold", false, { value: 10 }),
        plug("builtin.schedule.cooldown", false, { seconds: 10 }),
        plug("builtin.schedule.errorStatus", false),
        plug("builtin.schedule.streamPrelude", false, { ms: 60000 }),
        plug("builtin.schedule.streamSalvage", false),
        plug("builtin.schedule.streamContinue", false, { max: 10 }),
        plug("builtin.schedule.failover", false),
      ],
    },
    response: {
      mode: "process",
      chain: [
        plug("builtin.response.plaintextReasoning", plaintextMode === "auto", { mode: plaintextMode }),
        plug("builtin.response.crossResourceStrip", stripMode === "auto", { mode: stripMode }),
        plug("builtin.response.encryptedContentCleanup", false, { aggressive_strip: false }),
      ],
    },
  };
}

/** `config_json.pipeline` is the only source of truth. */
export function resolvePipeline(configJson: string, baseUrl = ""): PipelineConfig {
  try {
    const config = JSON.parse(configJson) as Record<string, unknown>;
    const raw = config.pipeline as PipelineConfig | undefined;
    if (raw && raw.version === 1 && raw.request && raw.response) return raw;
    return createDefaultPipeline(baseUrl);
  } catch {
    return createDefaultPipeline(baseUrl);
  }
}

function num(config: Record<string, unknown>, key: string, fallback: number): number {
  return typeof config[key] === "number" ? (config[key] as number) : fallback;
}

export function failurePolicyFromPipeline(
  pipeline: PipelineConfig,
  base: RouteCredentialFailurePolicy,
): RouteCredentialFailurePolicy {
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
}
