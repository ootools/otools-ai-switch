import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import {
  cancelPiiModelDownload,
  deletePiiModel,
  getPiiModelStatus,
  saveSettings,
  startPiiModelDownload,
  warmPiiModel,
} from "../../lib/api/client";
import type { AppSettings, PiiDownloadProgress } from "../../lib/api/types";
import { getTransport } from "../../lib/transport";

const stateLabels: Record<PiiDownloadProgress["state"], string> = {
  idle: "未开始",
  downloading: "下载中",
  verifying: "校验中",
  extracting: "解压中",
  ready: "就绪",
  failed: "失败",
  cancelled: "已取消",
};

function formatBytes(value: number) {
  if (!Number.isFinite(value) || value <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let index = 0;
  let amount = value;
  while (amount >= 1024 && index < units.length - 1) {
    amount /= 1024;
    index += 1;
  }
  return `${amount.toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
}

/** Download / runtime manager for the optional local PII model. */
export function PiiModelSettings({ settings }: { settings: AppSettings }) {
  const queryClient = useQueryClient();
  const [progress, setProgress] = useState<PiiDownloadProgress | null>(null);
  const [mirrorDraft, setMirrorDraft] = useState<string | null>(null);
  const [thresholdDraft, setThresholdDraft] = useState<number | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const statusQuery = useQuery({
    queryKey: ["pii-model-status"],
    queryFn: getPiiModelStatus,
    refetchInterval: 5000,
  });

  useEffect(() => {
    let alive = true;
    const unsubscribe: Array<() => void | Promise<void>> = [];
    void getTransport()
      .subscribe<PiiDownloadProgress>("pii-model-download-progress", (payload) => {
        if (alive) setProgress(payload);
      })
      .then((release) => unsubscribe.push(release))
      .catch(() => undefined);
    void getTransport()
      .subscribe<PiiDownloadProgress>("pii-model-download-status", (payload) => {
        if (alive) {
          setProgress(payload);
          void queryClient.invalidateQueries({ queryKey: ["pii-model-status"] });
        }
      })
      .then((release) => unsubscribe.push(release))
      .catch(() => undefined);
    return () => {
      alive = false;
      for (const release of unsubscribe) void release();
    };
  }, [queryClient]);

  const saveMutation = useMutation({
    mutationFn: saveSettings,
    onSuccess: (next) => {
      queryClient.setQueryData(["settings"], next);
      void queryClient.invalidateQueries({ queryKey: ["pii-model-status"] });
    },
  });
  const downloadMutation = useMutation({
    mutationFn: () => startPiiModelDownload(mirrorDraft ?? settings.pii_model_mirror_base_url),
    onSuccess: () => setNotice("已开始下载，进度见下方。"),
    onError: (error) => setNotice(error instanceof Error ? error.message : "启动下载失败。"),
  });
  const cancelMutation = useMutation({
    mutationFn: cancelPiiModelDownload,
    onSuccess: () => setNotice("已请求取消，未完成的临时文件会保留以便续传。"),
  });
  const deleteMutation = useMutation({
    mutationFn: deletePiiModel,
    onSuccess: () => {
      setNotice("已删除本地模型与运行时文件。");
      void queryClient.invalidateQueries({ queryKey: ["pii-model-status"] });
    },
  });
  const warmMutation = useMutation({
    mutationFn: warmPiiModel,
    onSuccess: () => {
      setNotice("模型已加载。");
      void queryClient.invalidateQueries({ queryKey: ["pii-model-status"] });
    },
    onError: (error) => setNotice(error instanceof Error ? error.message : "模型加载失败。"),
  });

  const status = statusQuery.data;
  const enabled = settings.pii_model_enabled ?? false;
  const mirror = mirrorDraft ?? settings.pii_model_mirror_base_url ?? "";
  const threshold = thresholdDraft ?? settings.pii_model_threshold_percent ?? 80;
  const busy = progress?.state === "downloading" || progress?.state === "verifying" || progress?.state === "extracting";

  return (
    <div className="mt-4 space-y-3 border-t border-stone-200 pt-4">
      <div>
        <h3 className="text-[13px] font-semibold text-stone-950">本地 PII 模型（可选）</h3>
        <p className="mt-1 text-[11px] leading-5 text-stone-500">
          AstrLink Guard int8（约 39 MB，CPU 运行，中英文）。规则和模型会同时生效：模型找出自由文本里的人名、地址，
          规则继续处理密钥和邮箱。模型与运行时不随安装包分发，启用后按需下载到本地数据目录。
        </p>
      </div>

      <label className="flex items-center gap-2 text-[13px] font-medium text-stone-700">
        <input
          aria-label="启用本地 PII 模型"
          checked={enabled}
          className="accent-blue-600"
          onChange={(event) => {
            saveMutation.mutate({ ...settings, pii_model_enabled: event.target.checked });
            if (event.target.checked && status?.file_state !== "ready") {
              setNotice("已启用。模型文件尚未就绪，请先下载；启用期间请求会被拦截。");
            }
          }}
          type="checkbox"
        />
        启用本地模型识别
      </label>

      <div className="grid gap-2 rounded-xl border border-stone-200 bg-stone-50 p-3 text-[12px] text-stone-600 sm:grid-cols-2">
        <p>支持状态：{status?.supported ? "当前平台可下载" : "当前平台不支持"}</p>
        <p>文件状态：{status?.file_state ?? "读取中"}</p>
        <p>运行状态：{status?.runtime_state ?? "读取中"}{status?.device ? `（${status.device}）` : ""}</p>
        <p>已下载：{formatBytes(status?.downloaded_bytes ?? 0)} / {formatBytes(status?.total_bytes ?? 0)}</p>
        <p>加载耗时：{status?.load_ms ? `${status.load_ms} ms` : "—"}</p>
        <p>下载进度：{progress ? `${stateLabels[progress.state]} ${progress.percent.toFixed(0)}%` : "—"}</p>
      </div>
      {status?.last_error ? (
        <p className="text-[12px] text-red-700" role="alert">{status.last_error}</p>
      ) : null}
      {progress?.artifact ? (
        <p className="break-all font-mono text-[11px] text-stone-500">{progress.artifact}</p>
      ) : null}
      {progress?.error ? (
        <p className="text-[12px] text-red-700" role="alert">{progress.error}</p>
      ) : null}

      <label className="flex flex-col gap-1.5 text-[12px] font-semibold text-stone-600">
        下载镜像地址（可选）
        <input
          aria-label="下载镜像地址"
          className="rounded-xl border border-stone-200 bg-white px-3 py-2 text-[12px] outline-none focus:border-blue-400"
          onChange={(event) => setMirrorDraft(event.target.value)}
          placeholder="https://mirror.example/pii-model"
          value={mirror}
        />
        <span className="text-[11px] font-normal text-stone-400">
          镜像只替换下载地址；文件仍按固定大小与 SHA-256 校验，不匹配就删除临时文件。
        </span>
      </label>

      <label className="flex flex-col gap-1.5 text-[12px] font-semibold text-stone-600">
        置信度阈值：{threshold}%
        <input
          aria-label="模型置信度阈值"
          className="accent-blue-600"
          max={99}
          min={50}
          onChange={(event) => setThresholdDraft(Number(event.target.value))}
          type="range"
          value={threshold}
        />
        <span className="text-[11px] font-normal text-stone-400">
          越高越保守（漏掉的真实信息更多），默认 80%。
        </span>
      </label>

      <div className="flex flex-wrap gap-2">
        <button
          className="rounded-xl border border-blue-200 bg-blue-50 px-3 py-2 text-[12px] font-semibold text-blue-900 motion-control hover:bg-blue-100 disabled:opacity-40"
          disabled={!status?.supported || busy}
          onClick={() => downloadMutation.mutate()}
          type="button"
        >
          {busy ? "下载中…" : status?.file_state === "ready" ? "重新校验/补齐" : "下载模型与运行时"}
        </button>
        <button
          className="rounded-xl border border-stone-300 bg-white px-3 py-2 text-[12px] font-semibold text-stone-700 motion-control hover:bg-stone-50 disabled:opacity-40"
          disabled={!busy}
          onClick={() => cancelMutation.mutate()}
          type="button"
        >
          取消下载
        </button>
        <button
          className="rounded-xl border border-stone-300 bg-white px-3 py-2 text-[12px] font-semibold text-stone-700 motion-control hover:bg-stone-50 disabled:opacity-40"
          disabled={status?.file_state !== "ready" || warmMutation.isPending}
          onClick={() => warmMutation.mutate()}
          type="button"
        >
          立即加载
        </button>
        <button
          className="rounded-xl border border-red-200 bg-white px-3 py-2 text-[12px] font-semibold text-red-700 motion-control hover:bg-red-50 disabled:opacity-40"
          disabled={status?.file_state === "missing" || deleteMutation.isPending || busy}
          onClick={() => deleteMutation.mutate()}
          type="button"
        >
          删除本地文件
        </button>
        <button
          className="rounded-xl border border-stone-300 bg-white px-3 py-2 text-[12px] font-semibold text-stone-700 motion-control hover:bg-stone-50"
          onClick={() =>
            saveMutation.mutate({
              ...settings,
              pii_model_mirror_base_url: mirror.trim() || null,
              pii_model_threshold_percent: Math.max(50, Math.min(99, threshold)),
            })
          }
          type="button"
        >
          保存镜像与阈值
        </button>
      </div>
      {notice ? <p className="text-[11px] text-stone-500" role="status">{notice}</p> : null}
      <p className="text-[11px] leading-5 text-stone-400">
        Windows 需要 Visual C++ 2019 运行库。模型启用但缺失或加载失败时，网关会拦截请求并提示到这里，不会把未脱敏原文发送出去。
      </p>
    </div>
  );
}
