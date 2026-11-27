import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen, Package, Trash2 } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import { ApiClientError } from "../../lib/api/errors";
import { apiErrorMessageKey } from "../../lib/api/errorMessages";
import {
  pluginPackGetDetail,
  pluginPackInstallFolder,
  pluginPackInstallUrl,
  pluginPackList,
  pluginPackSetEnabled,
  pluginPackUninstall,
} from "../../lib/api/client";
import { useI18n } from "../../lib/i18n";

const cardClass = "rounded-2xl border border-stone-200 bg-white p-4 shadow-sm";
const inputClass =
  "h-9 w-full rounded-lg border border-stone-200 bg-white px-3 text-[12px] text-stone-900 outline-none placeholder:text-stone-400 focus:border-blue-400";
const buttonClass =
  "inline-flex h-9 items-center gap-1.5 rounded-lg border border-stone-300 bg-white px-3 text-[12px] font-semibold text-stone-700 motion-control hover:bg-stone-50 disabled:cursor-not-allowed disabled:opacity-50";
const primaryClass =
  "inline-flex h-9 items-center gap-1.5 rounded-lg bg-stone-900 px-3 text-[12px] font-semibold text-white motion-control hover:bg-stone-800 disabled:cursor-not-allowed disabled:opacity-50";

export function PluginsSettings() {
  const queryClient = useQueryClient();
  const { t } = useI18n();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [url, setUrl] = useState("");
  const [sha256, setSha256] = useState("");

  const packsQuery = useQuery({ queryKey: ["plugin-packs"], queryFn: pluginPackList });
  const detailQuery = useQuery({
    queryKey: ["plugin-pack", selectedId],
    queryFn: () => pluginPackGetDetail(selectedId ?? ""),
    enabled: Boolean(selectedId),
  });

  const refresh = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["plugin-packs"] }),
      queryClient.invalidateQueries({ queryKey: ["plugin-pack"] }),
      queryClient.invalidateQueries({ queryKey: ["plugin-presets"] }),
    ]);
  };

  const installFolderMutation = useMutation({
    mutationFn: (path: string) => pluginPackInstallFolder(path),
    onSuccess: async (pack) => {
      setSelectedId(pack.id);
      await refresh();
    },
  });
  const installUrlMutation = useMutation({
    mutationFn: (input: { url: string; sha256: string }) =>
      pluginPackInstallUrl(input.url, input.sha256),
    onSuccess: async (pack) => {
      setSelectedId(pack.id);
      setUrl("");
      setSha256("");
      await refresh();
    },
  });
  const uninstallMutation = useMutation({
    mutationFn: (id: string) => pluginPackUninstall(id),
    onSuccess: async () => {
      setSelectedId(null);
      await refresh();
    },
  });
  const setEnabledMutation = useMutation({
    mutationFn: (input: { id: string; enabled: boolean }) =>
      pluginPackSetEnabled(input.id, input.enabled),
    onSuccess: refresh,
  });

  useEffect(() => {
    const packs = packsQuery.data ?? [];
    if (selectedId && packs.some((pack) => pack.id === selectedId)) return;
    setSelectedId(packs[0]?.id ?? null);
  }, [packsQuery.data, selectedId]);

  const error =
    packsQuery.error ??
    detailQuery.error ??
    installFolderMutation.error ??
    installUrlMutation.error ??
    uninstallMutation.error ??
    setEnabledMutation.error;
  const errorMessage =
    error instanceof ApiClientError ? t(apiErrorMessageKey(error.code)) : error ? String(error) : null;

  const busy =
    installFolderMutation.isPending ||
    installUrlMutation.isPending ||
    uninstallMutation.isPending ||
    setEnabledMutation.isPending;

  const pickFolder = async () => {
    try {
      const picked = await open({ directory: true, multiple: false });
      if (typeof picked === "string" && picked) {
        installFolderMutation.mutate(picked);
      }
    } catch {
      // A cancelled picker is not an error; a real failure surfaces as a
      // native error from the install command if the path is unusable.
    }
  };

  const submitUrl = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!url.trim() || !sha256.trim()) return;
    installUrlMutation.mutate({ url: url.trim(), sha256: sha256.trim() });
  };

  const packs = packsQuery.data ?? [];
  const detail = detailQuery.data;

  return (
    <div className="space-y-4">
      <div className={cardClass}>
        <div className="flex items-start gap-3">
          <span className="grid h-9 w-9 shrink-0 place-items-center rounded-xl bg-stone-900 text-white">
            <Package aria-hidden="true" className="h-4 w-4" />
          </span>
          <div className="min-w-0">
            <h2 className="text-[15px] font-semibold text-stone-950">
              {t("settings.plugins.title")}
            </h2>
            <p className="mt-1 text-[12px] leading-5 text-stone-500">
              {t("settings.plugins.subtitle")}
            </p>
          </div>
        </div>

        <div className="mt-4 flex flex-wrap items-center gap-2">
          <button
            aria-label={t("settings.plugins.install_folder")}
            className={buttonClass}
            disabled={busy}
            onClick={() => void pickFolder()}
            type="button"
          >
            <FolderOpen aria-hidden="true" className="h-3.5 w-3.5" />
            {t("settings.plugins.install_folder")}
          </button>
        </div>

        <form className="mt-3 grid gap-2" onSubmit={submitUrl}>
          <input
            aria-label={t("settings.plugins.install_url")}
            className={inputClass}
            onChange={(event) => setUrl(event.target.value)}
            placeholder={t("settings.plugins.url_placeholder")}
            type="url"
            value={url}
          />
          <input
            aria-label={t("settings.plugins.sha_placeholder")}
            className={`${inputClass} font-mono`}
            onChange={(event) => setSha256(event.target.value)}
            placeholder={t("settings.plugins.sha_placeholder")}
            type="text"
            value={sha256}
          />
          <div>
            <button
              className={primaryClass}
              disabled={busy || !url.trim() || !sha256.trim()}
              type="submit"
            >
              {installUrlMutation.isPending
                ? t("settings.plugins.installing")
                : t("settings.plugins.install")}
            </button>
          </div>
          <p className="text-[11px] leading-5 text-stone-400">
            {t("settings.plugins.install_hint")}
          </p>
        </form>
      </div>

      {errorMessage ? (
        <p className="rounded-xl border border-red-200 bg-red-50 px-3 py-2 text-[12px] font-semibold text-red-700" role="alert">
          {errorMessage}
        </p>
      ) : null}

      {packsQuery.isLoading ? (
        <p className="text-[12px] text-stone-500">…</p>
      ) : packs.length === 0 ? (
        <p className={`${cardClass} text-[12px] text-stone-500`}>
          {t("settings.plugins.empty")}
        </p>
      ) : (
        <ul className="list-none divide-y divide-stone-100 overflow-hidden rounded-2xl border border-stone-200 bg-white shadow-sm">
          {packs.map((pack) => (
            <li className="flex flex-wrap items-center gap-3 px-4 py-3" key={pack.id}>
              <button
                aria-label={pack.name}
                className="min-w-0 flex-1 text-left"
                onClick={() => setSelectedId(pack.id)}
                type="button"
              >
                <span className="flex flex-wrap items-center gap-2">
                  <span className="text-[13px] font-semibold text-stone-950">{pack.name}</span>
                  <span className="rounded-full bg-stone-100 px-2 py-0.5 font-mono text-[10px] text-stone-600">
                    v{pack.version}
                  </span>
                  <span
                    className={`rounded-full px-2 py-0.5 text-[10px] font-semibold ${
                      pack.enabled
                        ? "bg-emerald-50 text-emerald-800"
                        : "bg-stone-100 text-stone-600"
                    }`}
                  >
                    {pack.enabled
                      ? t("settings.plugins.enabled")
                      : t("settings.plugins.disabled")}
                  </span>
                </span>
                <span className="mt-0.5 block truncate text-[11px] text-stone-500">
                  {pack.id} · {pack.author} ·{" "}
                  {t(
                    pack.source === "url"
                      ? "settings.plugins.url"
                      : "settings.plugins.local",
                  )}{" "}
                  · {t("settings.plugins.presets", { count: pack.preset_count })}
                </span>
                <span className="mt-0.5 block truncate text-[11px] text-stone-400">
                  {pack.description}
                </span>
              </button>
              <label className="flex items-center gap-1.5 text-[11px] font-semibold text-stone-600">
                <input
                  aria-label={`${t("settings.plugins.enabled")} ${pack.name}`}
                  checked={pack.enabled}
                  className="h-3.5 w-3.5 accent-stone-900"
                  disabled={busy}
                  onChange={(event) =>
                    setEnabledMutation.mutate({ id: pack.id, enabled: event.target.checked })
                  }
                  type="checkbox"
                />
                {t("settings.plugins.enabled")}
              </label>
              <button
                aria-label={`${t("settings.plugins.uninstall")} ${pack.name}`}
                className="grid h-7 w-7 place-items-center rounded-lg border border-stone-200 text-stone-500 motion-control hover:bg-red-50 hover:text-red-700 disabled:opacity-50"
                disabled={busy}
                onClick={() => {
                  if (window.confirm(t("settings.plugins.uninstall_confirm", { name: pack.name }))) {
                    uninstallMutation.mutate(pack.id);
                  }
                }}
                title={t("settings.plugins.uninstall")}
                type="button"
              >
                <Trash2 aria-hidden="true" className="h-3.5 w-3.5" />
              </button>
            </li>
          ))}
        </ul>
      )}

      {selectedId && detail ? (
        <div className={cardClass}>
          <h3 className="text-[13px] font-semibold text-stone-950">
            {t("settings.plugins.detail")} · {detail.manifest.name}
          </h3>
          <p className="mt-1 break-all font-mono text-[11px] text-stone-500">{detail.path}</p>
          <p className="mt-2 text-[11px] text-stone-500">
            {detail.manifest.license} · {detail.manifest.author}
          </p>
          <div className="mt-3 space-y-1.5">
            {detail.manifest.contributes.provider_presets.length === 0 ? (
              <p className="text-[11px] text-stone-400">{t("settings.plugins.no_presets")}</p>
            ) : (
              detail.manifest.contributes.provider_presets.map((preset) => (
                <div
                  className="rounded-lg border border-stone-100 bg-stone-50 px-2.5 py-2"
                  key={preset.id}
                >
                  <p className="text-[12px] font-semibold text-stone-800">
                    {preset.label} · {preset.interface_format}
                  </p>
                  <p className="mt-0.5 break-all font-mono text-[10px] text-stone-500">
                    {preset.base_url}
                  </p>
                  <p className="mt-0.5 text-[10px] text-stone-400">
                    {t("settings.plugins.mappings", { count: preset.model_mappings.length })}
                  </p>
                </div>
              ))
            )}
          </div>
        </div>
      ) : null}
    </div>
  );
}
