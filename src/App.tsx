import { QueryClientProvider } from "@tanstack/react-query";
import { useCallback, useEffect, useRef, useState } from "react";
import { AnimatePresence } from "motion/react";
import { DeepLinkImportDialog } from "./components/deeplink/DeepLinkImportDialog";
import { LowDiskSpaceBanner } from "./components/system/LowDiskSpaceBanner";
import { AppLayout } from "./components/layout/AppLayout";
import {
  modalityForScreen,
  modalityScreenById,
  readModalityVisibility,
  resolveVisibleModalityScreen,
  writeModalityVisibility,
  type ModalityVisibility,
} from "./lib/accountModalities";
import { ThemeSync } from "./components/theme/ThemeSync";
import { I18nProvider } from "./lib/i18n";
import { createQueryClient } from "./lib/query/queryClient";
import { UsageOverviewPanel } from "./components/accounts/UsageOverviewPanel";
import { AboutScreen } from "./screens/AboutScreen";
import { BatchesScreen } from "./screens/BatchesScreen";
import { DashboardScreen } from "./screens/DashboardScreen";
import { ImportsScreen } from "./screens/ImportsScreen";
import { OperationLogScreen } from "./screens/OperationLogScreen";
import { CryptoToolsScreen } from "./screens/CryptoToolsScreen";
import { OcrScreen } from "./screens/OcrScreen";
import { ProvidersScreen } from "./screens/ProvidersScreen";
import { SessionsScreen } from "./screens/SessionsScreen";
import { SettingsScreen } from "./screens/SettingsScreen";
import { TargetsScreen } from "./screens/TargetsScreen";
import { McpScreen } from "./screens/McpScreen";
import { SkillsScreen } from "./screens/SkillsScreen";
import { MotionPage, MotionProvider, type MotionDirection } from "./components/motion/MotionPrimitives";
import { ImageGenerationScreen } from "./imagegen/ImageGenerationScreen";
import { ClientSetupScreen, type ClientSetupFocus } from "./screens/ClientSetupScreen";
import { CodexChannelsScreen } from "./screens/CodexChannelsScreen";

const queryClient = createQueryClient();

const implementedScreens = new Set([
  ...Object.values(modalityScreenById),
  "Dashboard",
  "Batches",
  "Providers",
  "Imports",
  "Targets",
  "CryptoTools",
  "OCR",
  "Settings",
  "ImageGen",
  "Sessions",
  "ClientSetup",
  "Log",
  "MCP",
  "Skills",
  "Stats",
  "About",
]);

export type PoolScopeFocus = {
  platform: string;
  scope: "in_pool" | "out_of_pool";
  nonce: number;
};

export function App() {
  const [screen, setScreen] = useState(modalityScreenById.general);
  const screenRef = useRef(modalityScreenById.general);
  const [navigationDirection, setNavigationDirection] = useState<MotionDirection>("neutral");
  const [sessionPlatform, setSessionPlatform] = useState<string | null>(null);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [poolScopeFocus, setPoolScopeFocus] = useState<PoolScopeFocus | null>(null);
  // 客户端接入页的深链（侧边栏 / 账号页工具栏 / 直连恢复），一次生效。
  const [clientSetupFocus, setClientSetupFocus] = useState<ClientSetupFocus | null>(null);
  // 「去调整映射」：跳到通用账号页并预置来源筛选，一次生效。
  const [accountProviderFocus, setAccountProviderFocus] = useState<{ provider: string; nonce: number } | null>(null);
  const [modalityVisibility, setModalityVisibility] = useState<ModalityVisibility>(readModalityVisibility);
  const accountModality = modalityForScreen(screen);

  useEffect(() => {
    writeModalityVisibility(modalityVisibility);
  }, [modalityVisibility]);

  useEffect(() => {
    const nextScreen = resolveVisibleModalityScreen(screen, modalityVisibility);
    if (nextScreen === screen) return;
    screenRef.current = nextScreen;
    setNavigationDirection("neutral");
    setScreen(nextScreen);
  }, [modalityVisibility, screen]);

  const navigate = (nextScreen: string) => {
    const screens = Array.from(implementedScreens);
    const previousIndex = screens.indexOf(screenRef.current);
    const nextIndex = screens.indexOf(nextScreen);
    setNavigationDirection(
      previousIndex === -1 || nextIndex === -1
        ? "neutral"
        : nextIndex >= previousIndex
          ? "forward"
          : "backward",
    );
    screenRef.current = nextScreen;
    if (nextScreen === "Sessions") {
      setSessionPlatform(null);
    }
    setScreen(nextScreen);
  };

  const openSessions = (platform?: string | null) => {
    setNavigationDirection("forward");
    screenRef.current = "Sessions";
    setSessionPlatform(platform ?? null);
    setScreen("Sessions");
  };

  const openClientSetup = useCallback((focus: ClientSetupFocus) => {
    setNavigationDirection("forward");
    screenRef.current = "ClientSetup";
    setClientSetupFocus(focus);
    setScreen("ClientSetup");
  }, []);
  const handleClientSetupFocusConsumed = useCallback(() => {
    setClientSetupFocus(null);
  }, []);
  const openAccountsWithProvider = useCallback((provider: string) => {
    setNavigationDirection("forward");
    screenRef.current = modalityScreenById.general;
    setAccountProviderFocus({ provider, nonce: Date.now() });
    setScreen(modalityScreenById.general);
  }, []);
  const handleProviderFilterFocusConsumed = useCallback((nonce: number) => {
    setAccountProviderFocus((current) => (current?.nonce === nonce ? null : current));
  }, []);

  const handleDeepLinkImported = useCallback(
    (_platform: string, options?: { joinedPool?: boolean }) => {
      setNavigationDirection("forward");
      screenRef.current = modalityScreenById.general;
      setScreen(modalityScreenById.general);
      // 所有导入账号都属于统一池；导入厂商不再决定导航入口。
      setPoolScopeFocus({
        platform: "codex",
        scope: options?.joinedPool ? "in_pool" : "out_of_pool",
        nonce: Date.now(),
      });
    },
    [],
  );
  const handlePoolScopeFocusConsumed = useCallback((nonce: number) => {
    setPoolScopeFocus((current) => (current?.nonce === nonce ? null : current));
  }, []);

  return (
    <QueryClientProvider client={queryClient}>
      <I18nProvider>
        <ThemeSync />
        <MotionProvider>
          <DeepLinkImportDialog onImported={handleDeepLinkImported} />
          <LowDiskSpaceBanner />
          <AppLayout
            activeScreen={screen}
            modalityVisibility={modalityVisibility}
            onModalityVisibilityChange={setModalityVisibility}
            onNavigate={navigate}
            onToggleSidebar={() => setSidebarCollapsed((value) => !value)}
            sidebarCollapsed={sidebarCollapsed}
          >
            <AnimatePresence initial={false} mode="wait">
              <MotionPage direction={navigationDirection} key={screen}>
                {accountModality && (
                  <CodexChannelsScreen
                    imageGenerationEnabled
                    hideSegmentTabs
                    modality={accountModality}
                    onOpenImageStudio={() => navigate("ImageGen")}
                    onOpenSessions={openSessions}
                    onOpenClientSetup={(platform, clientKey) => openClientSetup({ platform, clientKey })}
                    onPoolScopeFocusConsumed={handlePoolScopeFocusConsumed}
                    onProviderFilterFocusConsumed={handleProviderFilterFocusConsumed}
                    poolScopeFocus={poolScopeFocus}
                    providerFilterFocus={accountProviderFocus}
                    sidebarCollapsed={sidebarCollapsed}
                  />
                )}
                {screen === "ClientSetup" && (
                  <ClientSetupScreen
                    focus={clientSetupFocus}
                    onOpenAccounts={openAccountsWithProvider}
                    onFocusConsumed={handleClientSetupFocusConsumed}
                  />
                )}
                {screen === "Dashboard" && <DashboardScreen />}
                {screen === "Batches" && <BatchesScreen />}
                {screen === "Providers" && <ProvidersScreen />}
                {screen === "Imports" && <ImportsScreen />}
                {screen === "Targets" && <TargetsScreen />}
                {screen === "CryptoTools" && <CryptoToolsScreen />}
                {screen === "OCR" && <OcrScreen />}
                {screen === "Sessions" && <SessionsScreen initialPlatform={sessionPlatform} />}
                {screen === "Settings" && (
                  <SettingsScreen
                    modalityVisibility={modalityVisibility}
                    onModalityVisibilityChange={(modality, visible) =>
                      setModalityVisibility((current) => ({ ...current, [modality]: visible }))
                    }
                    onOpenFeature={navigate}
                  />
                )}
                {screen === "ImageGen" && <ImageGenerationScreen />}
                {screen === "MCP" && <McpScreen />}
                {screen === "Skills" && <SkillsScreen />}
                {screen === "Stats" && (
                  <section className="overflow-hidden rounded-xl border border-stone-200 bg-white shadow-sm">
                    <UsageOverviewPanel standalone />
                  </section>
                )}
                {screen === "About" && <AboutScreen />}
                {screen === "Log" && <OperationLogScreen />}
                {!implementedScreens.has(screen) && (
                  <div className="rounded-2xl border border-stone-200 bg-white/80 p-5 text-sm text-stone-500 shadow-sm">
                    {screen}
                  </div>
                )}
              </MotionPage>
            </AnimatePresence>
          </AppLayout>
        </MotionProvider>
      </I18nProvider>
    </QueryClientProvider>
  );
}
