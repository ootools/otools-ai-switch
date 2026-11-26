import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { motion } from "motion/react";
import {
  BarChart3,
  Bot,
  History,
  Info,
  Images,
  Menu,
  Network,
  PlugZap,
  Server,
  Settings2,
  Sparkles,
  type LucideIcon,
} from "lucide-react";
import { AiSwitchLogo } from "../brand/AiSwitchLogo";
import { AgentIcon, type AgentIconPlatform } from "../brand/AgentIcon";
import { modalityForScreen } from "../../lib/accountModalities";
import { useI18n } from "../../lib/i18n";
import { useDragResize } from "../../lib/useDragResize";

export const settingsFeatureScreens = [
  "Log",
  "Insights",
] as const;

const SIDEBAR_DEFAULT_WIDTH = 216;
const SIDEBAR_MIN_WIDTH = 180;
const SIDEBAR_MAX_WIDTH = 320;
const SIDEBAR_WIDTH_STORAGE_KEY = "ai-switch.sidebar-width";
const SIDEBAR_DRAWER_BREAKPOINT = 600;

function clampSidebarWidth(value: number) {
  return Math.min(Math.max(value, SIDEBAR_MIN_WIDTH), SIDEBAR_MAX_WIDTH);
}

function readSidebarWidth() {
  if (typeof window === "undefined") {
    return SIDEBAR_DEFAULT_WIDTH;
  }

  try {
    const rawValue = window.localStorage.getItem(SIDEBAR_WIDTH_STORAGE_KEY);
    if (!rawValue) {
      return SIDEBAR_DEFAULT_WIDTH;
    }
    const storedValue = Number(rawValue);
    return Number.isFinite(storedValue) ? clampSidebarWidth(storedValue) : SIDEBAR_DEFAULT_WIDTH;
  } catch {
    return SIDEBAR_DEFAULT_WIDTH;
  }
}

type AppLayoutProps = {
  children: ReactNode;
  activeScreen: string;
  onNavigate: (screen: string) => void;
  onToggleSidebar: () => void;
  sidebarCollapsed: boolean;
};

function isSettingsArea(screen: string) {
  return screen === "Settings" || (settingsFeatureScreens as readonly string[]).includes(screen);
}

function NavButton({
  collapsed,
  icon,
  label,
  active,
  onClick,
  variant = "standard",
}: {
  collapsed: boolean;
  icon: LucideIcon | AgentIconPlatform;
  label: string;
  active: boolean;
  onClick: () => void;
  variant?: "primary" | "standard";
}) {
  const baseClasses = `group flex w-full items-center rounded-xl border py-2 text-left text-[13px] motion-control duration-150 focus:outline-none focus-visible:ring-2 focus-visible:ring-blue-400 ${
    collapsed ? "justify-center px-0" : "justify-between px-3"
  }`;
  const activeClasses =
    variant === "primary"
      ? "border-stone-300 bg-white text-stone-950 shadow-sm"
      : "border-stone-300 bg-stone-100 text-stone-950 shadow-sm";
  const idleClasses =
    variant === "primary"
      ? "border-transparent bg-transparent text-stone-600 hover:bg-white/60 hover:text-stone-950"
      : "border-transparent bg-transparent text-stone-600 hover:bg-stone-100 hover:text-stone-950";
  const LucideIconComponent = typeof icon === "string" ? null : icon;

  return (
    <button
      aria-current={active ? "page" : undefined}
      className={`relative ${baseClasses} ${active ? activeClasses : idleClasses}`}
      onClick={onClick}
      title={label}
      type="button"
    >
      {active ? (
        <motion.span
          aria-hidden="true"
          className="pointer-events-none absolute inset-y-1 left-0.5 w-0.5 rounded-full bg-amber-500"
          layoutId="app-sidebar-active-indicator"
          transition={{ duration: 0.22, ease: [0.22, 1, 0.36, 1] }}
        />
      ) : null}
      <span
        className={`relative z-10 flex min-w-0 items-center ${collapsed ? "justify-center gap-0" : "gap-2"}`}
      >
        <span
          className={`h-1.5 w-1.5 shrink-0 rounded-full ${collapsed ? "hidden" : ""} ${
            active ? "bg-amber-500" : "bg-stone-300 group-hover:bg-stone-400"
          }`}
        />
        {typeof icon === "string" ? (
          <AgentIcon className="h-4 w-4" platform={icon} />
        ) : (
          LucideIconComponent ? (
            <LucideIconComponent aria-hidden="true" className={`h-4 w-4 shrink-0 ${active ? "text-amber-600" : "text-stone-500"}`} />
          ) : null
        )}
        <span className={`truncate font-medium ${collapsed ? "sr-only" : ""}`}>{label}</span>
      </span>
      <span
        aria-hidden="true"
        className={`${collapsed ? "hidden" : ""} ${
          active ? "text-stone-400" : "text-transparent"
        }`}
      >
        /
      </span>
    </button>
  );
}

export function AppLayout({
  children,
  activeScreen,
  onNavigate,
  onToggleSidebar,
  sidebarCollapsed,
}: AppLayoutProps) {
  const { t } = useI18n();
  const appShellRef = useRef<HTMLDivElement | null>(null);
  const [sidebarWidth, setSidebarWidth] = useState(readSidebarWidth);
  const [narrowLayout, setNarrowLayout] = useState(
    () => typeof window !== "undefined" && window.innerWidth < SIDEBAR_DRAWER_BREAKPOINT,
  );
  const [sidebarDrawerOpen, setSidebarDrawerOpen] = useState(false);
  const settingsActive = isSettingsArea(activeScreen);
  const accountWorkspaceActive = activeScreen === "Suppliers" || modalityForScreen(activeScreen) !== undefined;
  const sidebarDrawerVisible = narrowLayout && sidebarDrawerOpen;
  const sidebarContentCollapsed = narrowLayout ? !sidebarDrawerOpen : sidebarCollapsed;
  const desktopGridClass = sidebarCollapsed
    ? "min-[600px]:grid-cols-[56px_minmax(0,1fr)]"
    : "min-[600px]:grid-cols-[var(--app-sidebar-width)_minmax(0,1fr)]";
  const { dragging: sidebarResizing, startDragging: startSidebarResize } = useDragResize({
    axis: "x",
    min: SIDEBAR_MIN_WIDTH,
    max: SIDEBAR_MAX_WIDTH,
    getInitialValue: () => sidebarWidth,
    getValueFromPointer: (event) => {
      const shellRect = appShellRef.current?.getBoundingClientRect();
      return shellRect ? event.clientX - shellRect.left : sidebarWidth;
    },
    onChange: setSidebarWidth,
  });

  useEffect(() => {
    try {
      window.localStorage.setItem(SIDEBAR_WIDTH_STORAGE_KEY, String(clampSidebarWidth(sidebarWidth)));
    } catch {
      // Storage may be unavailable in restricted webviews.
    }
  }, [sidebarWidth]);

  useEffect(() => {
    const syncNarrowLayout = () => {
      const nextNarrowLayout = window.innerWidth < SIDEBAR_DRAWER_BREAKPOINT;
      setNarrowLayout(nextNarrowLayout);
      if (!nextNarrowLayout) {
        setSidebarDrawerOpen(false);
      }
    };
    syncNarrowLayout();
    window.addEventListener("resize", syncNarrowLayout);
    return () => window.removeEventListener("resize", syncNarrowLayout);
  }, []);

  useEffect(() => {
    if (!sidebarDrawerVisible) {
      return;
    }
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setSidebarDrawerOpen(false);
      }
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [sidebarDrawerVisible]);

  const handleToggleSidebar = () => {
    if (narrowLayout) {
      setSidebarDrawerOpen((current) => !current);
      return;
    }
    onToggleSidebar();
  };

  const handleNavigate = (nextScreen: string) => {
    setSidebarDrawerOpen(false);
    onNavigate(nextScreen);
  };

  return (
    <main className="box-border h-screen max-h-[100dvh] overflow-hidden text-stone-950">
      <div
        className={`app-shell box-border grid h-full min-h-0 grid-cols-[56px_minmax(0,1fr)] ${desktopGridClass}`}
        data-testid="app-shell"
        ref={appShellRef}
        style={
          {
            "--app-sidebar-width": `${sidebarContentCollapsed ? 56 : sidebarWidth}px`,
          } as CSSProperties
        }
      >
        {sidebarDrawerVisible && (
          <div
            aria-hidden="true"
            className="app-sidebar-drawer-backdrop"
            data-testid="app-sidebar-drawer-backdrop"
            onClick={() => setSidebarDrawerOpen(false)}
          />
        )}
        <aside
          className={`app-sidebar ${
            sidebarDrawerVisible ? "app-sidebar-drawer" : "relative"
          } flex h-full min-h-0 flex-col overflow-hidden border-r border-white/80 bg-gradient-to-br from-slate-50/92 via-emerald-50/74 to-amber-50/70 shadow-xl shadow-stone-900/5 backdrop-blur-2xl ${
            sidebarContentCollapsed ? "p-2" : "p-3"
          }`}
          data-testid="app-sidebar"
        >
          <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(circle_at_20%_15%,rgba(16,185,129,0.18),transparent_34%),radial-gradient(circle_at_88%_8%,rgba(245,158,11,0.16),transparent_30%),linear-gradient(180deg,rgba(255,255,255,0.72),rgba(255,255,255,0.38))]" />
          <div className="relative flex min-h-0 flex-1 flex-col">
            <div
              className={`flex justify-between rounded-2xl border border-white/80 bg-white/56 shadow-sm backdrop-blur-xl ${
                sidebarContentCollapsed
                  ? "mb-4 flex-col items-center gap-2 p-1"
                  : "mb-5 items-start gap-3 p-3"
              }`}
            >
              <div
                className={`min-w-0 items-center gap-2 ${
                  sidebarContentCollapsed ? "hidden" : "flex"
                }`}
              >
                <AiSwitchLogo className="h-9 w-9 shrink-0 rounded-2xl shadow-sm" />
                <div className="min-w-0">
                  <p className="truncate text-[13px] font-semibold text-stone-950">AI Switch</p>
                  <p className="truncate text-[11px] text-stone-500">{t("layout.brandBadge")}</p>
                </div>
              </div>
              <div
                className={`flex items-center gap-2 ${
                  sidebarContentCollapsed ? "flex-col" : ""
                }`}
              >
                <button
                  aria-expanded={!sidebarContentCollapsed}
                  aria-label={
                    sidebarContentCollapsed
                      ? t("layout.expandSidebar")
                      : t("layout.collapseSidebar")
                  }
                  className="grid h-8 w-8 shrink-0 place-items-center rounded-xl border border-stone-200 bg-white/70 text-stone-600 shadow-sm motion-control hover:border-stone-300 hover:bg-white hover:text-stone-950 focus:outline-none focus-visible:ring-2 focus-visible:ring-blue-400"
                  onClick={handleToggleSidebar}
                  title={
                    sidebarContentCollapsed
                      ? t("layout.expandSidebar")
                      : t("layout.collapseSidebar")
                  }
                  type="button"
                >
                  <Menu aria-hidden="true" className="h-4 w-4" />
                </button>
              </div>
            </div>

            <div className="min-h-0 flex-1 space-y-1 overflow-y-auto pr-0.5">
              <NavButton
                active={activeScreen === "Suppliers" || modalityForScreen(activeScreen) !== undefined}
                collapsed={sidebarContentCollapsed}
                icon="codex"
                label={t("nav.suppliers")}
                onClick={() => handleNavigate("Suppliers")}
                variant="primary"
              />
              <NavButton
                active={activeScreen === "ClientSetup"}
                collapsed={sidebarContentCollapsed}
                icon={Bot}
                label={t("nav.agents")}
                onClick={() => handleNavigate("ClientSetup")}
              />
              <NavButton
                active={activeScreen === "Gateway"}
                collapsed={sidebarContentCollapsed}
                icon={Server}
                label={t("nav.gateway")}
                onClick={() => handleNavigate("Gateway")}
              />
              <NavButton
                active={activeScreen === "Sessions"}
                collapsed={sidebarContentCollapsed}
                icon={History}
                label={t("nav.sessions")}
                onClick={() => handleNavigate("Sessions")}
              />
              <NavButton
                active={activeScreen === "Traffic"}
                collapsed={sidebarContentCollapsed}
                icon={Network}
                label={t("nav.traffic")}
                onClick={() => handleNavigate("Traffic")}
              />
              <NavButton
                active={activeScreen === "Stats"}
                collapsed={sidebarContentCollapsed}
                icon={BarChart3}
                label={t("nav.stats")}
                onClick={() => handleNavigate("Stats")}
              />
              <NavButton
                active={activeScreen === "ImageGen"}
                collapsed={sidebarContentCollapsed}
                icon={Images}
                label={t("nav.imagegen")}
                onClick={() => handleNavigate("ImageGen")}
              />
              <NavButton
                active={activeScreen === "MCP"}
                collapsed={sidebarContentCollapsed}
                icon={PlugZap}
                label={t("nav.mcp")}
                onClick={() => handleNavigate("MCP")}
              />
              <NavButton
                active={activeScreen === "Skills"}
                collapsed={sidebarContentCollapsed}
                icon={Sparkles}
                label={t("nav.skills")}
                onClick={() => handleNavigate("Skills")}
              />
              <NavButton
                active={settingsActive}
                collapsed={sidebarContentCollapsed}
                icon={Settings2}
                label={t("nav.settings")}
                onClick={() => handleNavigate("Settings")}
              />
              <NavButton
                active={activeScreen === "About"}
                collapsed={sidebarContentCollapsed}
                icon={Info}
                label={t("nav.about")}
                onClick={() => handleNavigate("About")}
              />
            </div>
          </div>
          {!narrowLayout && !sidebarCollapsed && (
            <div
              aria-label={t("layout.resizeSidebar")}
              aria-orientation="vertical"
              aria-valuemax={SIDEBAR_MAX_WIDTH}
              aria-valuemin={SIDEBAR_MIN_WIDTH}
              aria-valuenow={sidebarWidth}
              className={`absolute inset-y-0 right-0 z-20 w-1.5 touch-none select-none cursor-col-resize bg-transparent motion-control hover:bg-stone-300/70 ${
                sidebarResizing ? "bg-blue-400/70" : ""
              }`}
              data-testid="sidebar-resize-handle"
              onPointerDown={startSidebarResize}
              role="separator"
              title={t("layout.resizeSidebar")}
            />
          )}
        </aside>

        <section
          className={`col-start-2 box-border h-full min-h-0 min-w-0 bg-stone-100 ${
            accountWorkspaceActive || activeScreen === "ClientSetup" ? "overflow-hidden p-0"
              : activeScreen === "Traffic" ? "overflow-hidden p-2 sm:p-3" : "overflow-y-auto p-2 sm:p-3"
          }`}
        >
          {children}
        </section>
      </div>
    </main>
  );
}
