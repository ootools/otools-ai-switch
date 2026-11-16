import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useState, type CSSProperties } from "react";
import { ArrowLeft, ArrowRightLeft, FlaskConical, RefreshCw, Radio, Server, ShieldCheck } from "lucide-react";
import { AgentLogo } from "../components/brand/AgentLogo";
import { AiSwitchLogo } from "../components/brand/AiSwitchLogo";
import {
  getRouteProxyPoolMemberIds, getRouteProxyStatus, listConfigWriteClients,
  listPlatformCapabilities, listRouteProxyAccountSummaries,
} from "../lib/api/client";
import { agentClientInfo } from "../lib/agentClients";
import { useI18n } from "../lib/i18n";
import { buildTrafficModel, type TrafficCatalog, type TrafficClient } from "../lib/trafficState";
import { useTrafficActivity } from "../lib/useTrafficActivity";
import { buildTrafficSimulation } from "../lib/trafficSimulation";
import "./TrafficScreen.css";

async function loadTrafficCatalog(): Promise<TrafficCatalog & { warnings: string[] }> {
  const platforms = (await listPlatformCapabilities())
    .filter((row) => row.operations.generic_api_routing.availability !== "unavailable");
  const [memberIds, summaries] = await Promise.all([
    getRouteProxyPoolMemberIds(), listRouteProxyAccountSummaries("codex"),
  ]);
  const members = new Set(memberIds);
  const results = await Promise.allSettled(platforms.map(async ({ platform }) => ({
    platform, clients: await listConfigWriteClients(platform),
  })));
  const clients = new Map<string, TrafficClient>();
  const accounts = new Map(summaries.filter((row) => members.has(row.id)).map((row) => [row.id, row]));
  const warnings: string[] = [];
  results.forEach((result, index) => {
    if (result.status === "rejected") {
      warnings.push(`${platforms[index].display_name}: ${String(result.reason)}`);
      return;
    }
    const catalog = result.value;
    for (const client of catalog.clients) {
      const current = clients.get(client.client_key);
      if (current) {
        if (!current.platforms.includes(catalog.platform)) current.platforms.push(catalog.platform);
        if (client.file_status === "managed") current.file_status = "managed";
      } else {
        clients.set(client.client_key, {
          client_key: client.client_key, display_name: client.display_name,
          platforms: [catalog.platform], file_status: client.file_status,
        });
      }
    }
  });
  return { clients: [...clients.values()], accounts: [...accounts.values()], account_summaries: summaries, warnings };
}

function FlowPath({ d, color, delay = 0, pair }: {
  d: string; color: string; delay?: number; pair?: string;
}) {
  return (
    <g data-active-pair={pair} data-link-state="active" className="traffic-active-path" style={{ color }}>
      <path d={d} />
      <circle className="traffic-dot" r="3.4" style={{ offsetPath: `path("${d}")`, animationDelay: `${delay}s` }} />
      <circle className="traffic-dot traffic-dot-secondary" r="2.3" style={{ offsetPath: `path("${d}")`, animationDelay: `${delay - 1.4}s` }} />
    </g>
  );
}

export function TrafficScreen({ onBack }: { onBack?: () => void }) {
  const { t } = useI18n();
  const catalog = useQuery({
    queryKey: ["traffic-catalog"], queryFn: loadTrafficCatalog,
    staleTime: 0, refetchOnMount: "always",
  });
  const status = useQuery({
    queryKey: ["route-proxy-status"], queryFn: getRouteProxyStatus,
    refetchInterval: 5000, refetchIntervalInBackground: false,
  });
  const activity = useTrafficActivity();
  const [simulationFrame, setSimulationFrame] = useState<number | null>(null);
  const simulating = simulationFrame !== null;
  useEffect(() => {
    if (!simulating) return;
    const interval = window.setInterval(() => {
      if (document.visibilityState !== "hidden") {
        setSimulationFrame((frame) => frame === null ? null : frame + 1);
      }
    }, 1800);
    return () => window.clearInterval(interval);
  }, [simulating]);
  const simulation = useMemo(() => simulationFrame === null ? null : buildTrafficSimulation(
    catalog.data, simulationFrame, (number) => t("traffic.demoAccount", { number }),
  ), [catalog.data, simulationFrame, t]);
  const confirmed = simulating || (activity.snapshot !== null && !activity.error);
  const model = useMemo(() => buildTrafficModel(
    simulation?.catalog ?? catalog.data ?? { clients: [], accounts: [] },
    simulation?.active ?? activity.snapshot?.active ?? [],
  ), [catalog.data, activity.snapshot, simulation]);
  const ready = Boolean(status.data?.running && status.data.route_access_enabled !== false);
  const animated = simulating || (confirmed && ready);
  const height = Math.max(440, model.clients.length * 48 + 100, model.accounts.length * 64 + 110);
  const hubY = height / 2;
  const clientY = (index: number) => 72 + index * 48;
  const accountY = (index: number) => model.accounts.length < 2 ? hubY
    : 92 + index * (height - 184) / (model.accounts.length - 1);
  const clientPath = (index: number) => `M 230 ${clientY(index)} C 315 ${clientY(index)}, 337 ${hubY}, 415 ${hubY}`;
  const accountPath = (index: number, offset = 0) => `M 585 ${hubY + offset} C 670 ${hubY + offset}, 690 ${accountY(index)}, 770 ${accountY(index)}`;
  const error = [activity.error, catalog.error?.message, status.error?.message, ...(catalog.data?.warnings ?? [])]
    .filter(Boolean).join(" · ");
  const refresh = () => {
    void catalog.refetch();
    void status.refetch();
    activity.refresh();
  };

  return (
    <section className="traffic-screen" aria-label={t("traffic.title")} data-traffic-mode={simulating ? "simulation" : "live"}>
      <header className="traffic-heading">
        <div className="traffic-heading-copy">
          <span className="traffic-kicker"><Radio size={13} /> LIVE TOPOLOGY</span>
          <h1>{t("traffic.title")}</h1>
          <p>{t("traffic.subtitle")}</p>
        </div>
        <div className="traffic-toolbar">
          {onBack && <button type="button" onClick={onBack}><ArrowLeft size={15} />{t("traffic.accounts")}</button>}
          <button
            type="button"
            aria-pressed={simulating}
            className={simulating ? "traffic-simulation-active" : undefined}
            onClick={() => setSimulationFrame((frame) => frame === null ? 0 : null)}
            title={t("traffic.simulationHint")}
          >
            <FlaskConical size={15} />{t(simulating ? "traffic.stopSimulation" : "traffic.simulate")}
          </button>
          <button type="button" onClick={refresh} aria-label={t("traffic.refresh")}>
            <RefreshCw size={15} className={catalog.isFetching ? "traffic-refreshing" : undefined} />{t("traffic.refresh")}
          </button>
        </div>
      </header>

      {simulating && <div className="traffic-simulation-notice" role="status">
        <FlaskConical size={15} /><strong>{t("traffic.simulationMode")}</strong><span>{t("traffic.simulationHint")}</span>
      </div>}
      {error && <div className="traffic-error" role="alert">{error}</div>}

      <div className="traffic-network-panel">
        <div className="traffic-network-labels"><span>{t("traffic.clients")}</span><span>{t("traffic.gateway")}</span><span>{t("traffic.pool")} <small>{t("traffic.sorted")}</small></span></div>
        <div className="traffic-network-scroll">
          <div className="traffic-network" style={{ height }}>
            <svg className="traffic-links" viewBox={`0 0 1000 ${height}`} preserveAspectRatio="none" aria-hidden="true">
              {model.clients.map((client, index) => <path key={`client-${client.client_key}`} className="traffic-idle-path" data-link-state="idle" d={clientPath(index)} />)}
              {model.accounts.map((account, index) => <path key={`account-${account.id}`} className="traffic-idle-path" data-link-state="idle" d={accountPath(index)} />)}
              {animated && model.clients.map((client, index) => client.active_count > 0 && <FlowPath key={`flow-${client.client_key}`} d={clientPath(index)} color={agentClientInfo(client.client_key).color} delay={-index * 0.23} />)}
              {animated && model.edges.map((edge, index) => {
                const accountIndex = model.accounts.findIndex((account) => account.id === edge.credential_id);
                return accountIndex < 0 ? null : <FlowPath key={JSON.stringify([edge.client_key, edge.credential_id])} d={accountPath(accountIndex, (index % 5 - 2) * 3)} color={agentClientInfo(edge.client_key).color} delay={-index * 0.29} pair={`${edge.client_key}:${edge.credential_id}`} />;
              })}
            </svg>

            {model.clients.map((client, index) => {
              const info = agentClientInfo(client.client_key);
              return <div key={client.client_key} data-testid="traffic-client" className={`traffic-node traffic-client ${animated && client.active_count > 0 ? "traffic-node-active" : ""}`} style={{ top: clientY(index) - 20, "--client-color": info.color } as CSSProperties} title={client.display_name}>
                <AgentLogo clientKey={client.client_key} className="traffic-client-logo" />
                <div className="traffic-node-copy"><strong>{client.client_key === "unknown" ? t("traffic.unknown") : client.display_name}</strong><span>{client.active_count > 0 && confirmed ? t("traffic.activeRequests", { count: client.active_count }) : t("traffic.idle")}</span></div>
                <span className={`traffic-count ${animated && client.active_count > 0 ? "traffic-count-active" : ""}`}>{confirmed ? client.active_count : "—"}</span>
                <i className="traffic-port" />
              </div>;
            })}

            <div className={`traffic-hub ${animated && model.total_attempts > 0 ? "traffic-hub-active" : ""}`} style={{ top: hubY - 97 }}>
              <span className="traffic-hub-orbit" />
              <AiSwitchLogo className="traffic-hub-logo" />
              <h2>AI Switch</h2><p>{t("traffic.gateway")}</p>
              <div className="traffic-hub-count"><ArrowRightLeft size={14} />{confirmed ? model.total_attempts : "—"}<small>{t(simulating ? "traffic.simulationMode" : "traffic.inFlight")}</small></div>
              <div className={`traffic-hub-status ${ready ? "traffic-online" : "traffic-offline"}`}><i />{ready ? t("traffic.online") : t("traffic.stopped")}</div>
            </div>

            {model.accounts.map((account, index) => {
              const edges = model.edges.filter((edge) => edge.credential_id === account.id);
              const activeColor = edges[0] ? agentClientInfo(edges[0].client_key).color : "#64748b";
              return <div key={account.id} data-testid="traffic-account" className={`traffic-node traffic-account ${animated && account.active_count > 0 ? "traffic-node-active" : ""}`} style={{ top: accountY(index) - 24, "--client-color": activeColor } as CSSProperties} title={account.display_name}>
                <i className="traffic-port" />
                <span className="traffic-account-icon"><Server size={18} /></span>
                <div className="traffic-node-copy"><strong>{account.display_name}</strong><span>{account.active_count > 0 && confirmed ? t("traffic.concurrency", { count: account.active_count }) : t("traffic.idle")}<small> · P{account.route_priority}</small></span></div>
                <div className="traffic-account-count"><strong>{confirmed ? account.active_count : "—"}</strong><small>{account.max_concurrency > 0 ? `/ ${account.max_concurrency}` : ""}</small></div>
                {edges.length > 0 && <div className="traffic-account-clients">{edges.map((edge) => <span key={edge.client_key} style={{ background: agentClientInfo(edge.client_key).color }} title={`${agentClientInfo(edge.client_key).label} × ${edge.count}`} />)}</div>}
              </div>;
            })}

            {catalog.isPending && !simulating && <p className="traffic-placeholder">{t("traffic.syncing")}</p>}
            {!catalog.isPending && model.accounts.length === 0 && <p className="traffic-empty-pool">{t("traffic.emptyPool")}</p>}
          </div>
        </div>
        <footer className="traffic-network-footer">
          <div className="traffic-legend"><span><i className="traffic-legend-static" />{t("traffic.legendStatic")}</span><span><i className="traffic-legend-active" />{t(simulating ? "traffic.simulatedLegend" : "traffic.legendActive")}</span></div>
          <span className="traffic-security-note"><ShieldCheck size={13} />{t("traffic.readOnly")}</span>
        </footer>
      </div>
      <p className="traffic-footnote">{t("traffic.legacyHint")}</p>
    </section>
  );
}
