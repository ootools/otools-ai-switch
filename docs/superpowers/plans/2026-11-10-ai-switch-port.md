# AI Switch OTools Implementation Plan

> **For agentic workers:** implement assigned tasks within their listed file ownership. Use test-driven-development for changed behavioral boundaries; retain upstream regression tests. No commits or publication are authorized.

**Goal:** Deliver an independent panel-mode AI Switch plugin using the current OTools SDK, with an AI-only listener on port 10888.

**Architecture:** Copy and adapt the upstream React and Rust business source into this independent plugin. The SDK owns host transport selection; a native cdylib owns SQLite, routing and all background work.

**Tech Stack:** React 18, TypeScript, Vite, UnoCSS, Vitest, Rust, Tokio, Axum, SQLx SQLite, OTools SDK.

**Spec:** `docs/superpowers/specs/2026-11-10-ai-switch-design.md`

## Global Constraints

- Plugin identifier/package name: `otools-ai-switch`; display name: AI Switch.
- `entry: dist/index.html`, `openMode: panel`, no directLaunch.
- Default AI listener `127.0.0.1:10888`; no management HTTP routes, original Web shell, PTY/Vibe, Tailscale or ai-switch plugin runtime.
- Data isolated under `~/.otools/local/otools-ai-switch/`.
- Preserve licenses and unmodified historical migrations; do not modify original ai-switch or unrelated worktrees.
- Frontend commands use official invoke/listen routed by vendored SDK. No ai-switch web transport.
- Preserve image generation independently of plugin enablement. Exclude SaaS local site and administration.
- Native contract `{ok:true,data}` or `{ok:false,error}`; event poll returns `{events:[{topic,payload}]}`.
- Native library name `otools_ai_switch_native`, output staged as `lib/Windows.dll`, `lib/macOS.dylib` or `lib/Linux.so`.

## Review Focus

- Host bridge may arrive after module load; SDK must own transport readiness.
- AI default port conflicts must report accurately; invalid API keys cannot bypass auth via platform headers.
- Page closure must not kill the proxy; plugin shutdown must release runtime, listeners and subprocesses.
- Native file operations in a remote browser target the host; original desktop-only feature guards must be adapted.
- Packaged build must be independent of neighboring source checkouts and must omit excluded runtimes.

## Task 1: Frontend plugin adaptation

**Files:** `src/`, `tests/`, `public/`, frontend configs, `package.json`, `pnpm-lock.yaml`, `index.html`.

**Consumes:** vendored SDK at `vendor/otools-plugin-sdk/src/vite.ts`; native command names from retained upstream services.
**Produces:** a typechecked React plugin UI building to `dist/` using `__OTOOLS_PLUGIN_UUID__ = "otools-ai-switch"`.

- [ ] Copy required frontend source and retained tests.
- [ ] Add failing transport tests for SDK invoke, payload/error handling and event unsubscribe.
- [ ] Replace Tauri/Web runtime switch with official SDK-aliased invoke/listen.
- [ ] Remove shell, update/autostart, Vibe, web/Tailscale, plugin manager and SaaS navigation and imports. Keep image generation, MCP/Skills and session browsing.
- [ ] Adapt client setup/start/stop/status to pure AI proxy commands and port 10888; remove terminal actions.
- [ ] Run frontend tests, typecheck and Vite build; report every remaining failure.

## Task 2: Native plugin core

**Files:** `native/` exclusively.

**Consumes:** upstream `src-tauri` core and migration history; OTools native ABI and event poll contract.
**Produces:** `otools_ai_switch_native` cdylib; all retained management commands plus `poll_events`, `plugin_http_port`, `__otools_shutdown__`.

- [ ] Copy source, migrations and required business resources without build artifacts or desktop binaries.
- [ ] Add lifecycle/dispatch/AI-only auth boundary tests before adapting behavior.
- [ ] Remove desktop/server entrypoints, terminal/Tailscale/plugin runtime and SaaS coupling. Reuse dispatcher logic without mounting management HTTP routes.
- [ ] Inject plugin-private paths, implement persistent runtime/state, cancellation, native event queue and guarded FFI with matching allocation/free.
- [ ] Reuse text/image/audio/video protocol routes in an authenticated AI-only listener at 10888; do not mutate host proxy environment.
- [ ] Preserve retained desktop-only business commands (OAuth, exports, diagnostics) through native dispatch.
- [ ] Run cargo check, focused lifecycle/integration tests and applicable retained unit tests; build cdylib.

## Task 3: Independent package and acceptance

**Files:** `vendor/`, `plugin.json`, `logo.svg`, `scripts/`, licenses/provenance, `README.md`, `.gitignore`, `.github/` if needed.

**Consumes:** Task 1 dist output and Task 2 native library.
**Produces:** validated `.oplg` ZIP with frontend, native and business resources; build and installation documentation.

- [ ] Pin the actual current public SDK source and its license/provenance.
- [ ] Write manifest and platform build/stage/pack/verify scripts; package only dist, lib, manifest, logo, notices and needed business resources.
- [ ] Verify SDK/native command coverage, removal boundaries and default port/panel settings.
- [ ] Build Windows plugin, inspect archive, test actual native load/invoke/stop against isolated temporary data and mock upstream if possible.
- [ ] Review implementation independently, resolve actionable findings and document tested versus unavailable environment coverage.
