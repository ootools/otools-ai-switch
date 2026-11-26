import { Loader2, X } from "lucide-react";
import { useEffect, useId, useState } from "react";
import {
  detectSubscriptionLogin,
  importSubscriptionLogin,
} from "../../lib/api/client";
import type { ImportedCodexAccount, SubscriptionVendor } from "../../lib/api/types";

const vendors: Array<{ value: SubscriptionVendor; label: string; hint: string }> = [
  { value: "claude", label: "Claude", hint: "~/.claude/.credentials.json" },
  { value: "gemini", label: "Gemini", hint: "~/.gemini/oauth_creds.json" },
  { value: "grok", label: "Grok", hint: "~/.grok/auth.json" },
  { value: "copilot", label: "Copilot", hint: "~/.copilot/auth.json" },
];

const buttonClass =
  "inline-flex items-center justify-center gap-2 rounded-xl border border-stone-200 px-3 py-2 text-[13px] font-semibold text-stone-700 motion-control hover:bg-stone-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue-500 disabled:cursor-not-allowed disabled:opacity-50";

function subscriptionError(error: unknown, fallback: string): string {
  const code = error && typeof error === "object" && "code" in error ? String(error.code) : "";
  const messages: Record<string, string> = {
    validation_subscription_login_not_found: "未找到该厂商的登录文件。请先用厂商 CLI 登录，再重试。",
    validation_subscription_login_invalid: "登录文件格式无法识别，请确认该文件由厂商 CLI 写入。",
    validation_batch_name_required: "请先填写批量名称。",
    codex_oauth_import_failed: "账号导入失败，本次批量未保存，请重试。",
  };
  return messages[code] ?? fallback;
}

/**
 * Import a subscription the user already logged into with its own CLI.
 * Interactive in-app login is still Codex-only; this dialog deliberately does
 * not rewrite or refresh the source tool's credentials file.
 */
export function SubscriptionLoginDialog({
  batchName,
  onClose,
  onImported,
}: {
  batchName: string;
  onClose: () => void;
  onImported: (accounts: ImportedCodexAccount[]) => Promise<void> | void;
}) {
  const titleId = useId();
  const [vendor, setVendor] = useState<SubscriptionVendor>("claude");
  const [detected, setDetected] = useState<string | null>(null);
  const [detecting, setDetecting] = useState(false);
  const [importing, setImporting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    setDetecting(true);
    setError(null);
    setNotice(null);
    detectSubscriptionLogin(vendor)
      .then((path) => {
        if (active) setDetected(path);
      })
      .catch((cause: unknown) => {
        if (active) setError(subscriptionError(cause, "检测登录文件失败。"));
      })
      .finally(() => {
        if (active) setDetecting(false);
      });
    return () => {
      active = false;
    };
  }, [vendor]);

  const doImport = async () => {
    if (!batchName.trim()) {
      setError("请先填写批量名称。");
      return;
    }
    setImporting(true);
    setError(null);
    setNotice(null);
    try {
      const accounts = await importSubscriptionLogin(vendor, batchName.trim(), detected);
      await onImported(accounts);
      setNotice(`已导入 ${accounts.length} 个账号。`);
    } catch (cause) {
      setError(subscriptionError(cause, "导入订阅登录失败。"));
    } finally {
      setImporting(false);
    }
  };

  const activeVendor = vendors.find((item) => item.value === vendor) as (typeof vendors)[number];

  return (
    <div
      className="motion-overlay fixed inset-0 z-[60] grid place-items-center bg-stone-950/35 p-4 backdrop-blur-sm"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        aria-labelledby={titleId}
        aria-modal="true"
        className="max-h-[90vh] w-full max-w-lg overflow-y-auto rounded-2xl border border-stone-200 bg-white p-5 shadow-2xl"
        role="dialog"
      >
        <div className="flex items-center justify-between gap-4">
          <h3 className="text-lg font-semibold text-stone-950" id={titleId}>
            订阅登录接入
          </h3>
          <button
            aria-label="关闭订阅登录接入"
            className={buttonClass}
            onClick={onClose}
            type="button"
          >
            <X className="h-4 w-4" />
          </button>
        </div>
        <p className="mt-3 text-[13px] leading-6 text-stone-600">
          先用厂商 CLI 在本机登录，再在这里一键导入。导入只读凭据文件，不会把当前账号踢下线，也不会写回来源工具。
        </p>
        <p className="mt-2 break-all text-[13px] text-stone-600">
          导入批量：<span className="font-semibold text-stone-900">{batchName.trim() || "未填写"}</span>
        </p>
        <div className="mt-4 grid grid-cols-4 gap-2">
          {vendors.map((item) => (
            <button
              aria-pressed={vendor === item.value}
              className={`${buttonClass} text-[12px] ${
                vendor === item.value ? "border-blue-300 bg-blue-50 text-blue-900" : ""
              }`}
              key={item.value}
              onClick={() => setVendor(item.value)}
              type="button"
            >
              {item.label}
            </button>
          ))}
        </div>
        <div className="mt-4 rounded-xl border border-stone-200 bg-stone-50 px-3 py-2 text-[12px] text-stone-600">
          <p className="font-semibold text-stone-800">检测到的登录文件</p>
          <p className="mt-1 break-all font-mono text-[11px]">
            {detecting ? "检测中…" : (detected ?? `未找到（常见位置 ${activeVendor.hint}）`)}
          </p>
        </div>
        <p className="mt-3 text-xs leading-5 text-stone-500">
          Google 自 2026-06 起停止向个人 AI Pro / Ultra 订阅提供 Gemini CLI OAuth；Standard / Enterprise 订阅仍可导入。
          Copilot 以统一池身份存储并标注 auth_kind=copilot，当前不作为独立可路由厂商。
        </p>
        {notice ? <p className="mt-3 text-xs text-emerald-700" role="status">{notice}</p> : null}
        {error ? (
          <p className="mt-4 rounded-xl border border-red-200 bg-red-50 p-3 text-[13px] leading-5 text-red-700" role="alert">
            {error}
          </p>
        ) : null}
        <div className="mt-5 flex justify-end gap-2">
          <button className={buttonClass} onClick={onClose} type="button">关闭</button>
          <button
            className={`${buttonClass} border-blue-200 bg-blue-600 text-white hover:bg-blue-700`}
            disabled={detecting || importing}
            onClick={() => void doImport()}
            type="button"
          >
            {importing ? <Loader2 className="h-4 w-4 animate-spin motion-reduce:animate-none" /> : null}
            导入登录
          </button>
        </div>
      </div>
    </div>
  );
}
