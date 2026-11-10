# Validation record

Validated locally on Windows x64 with Node 22.22.2, pnpm 10.33.2, Rust 1.94.1 and Python 3.12.10.

## Automated checks

- Complete frontend suite after SDK remote dialogs, panel cleanup, image-session form and sidebar removal: **72 files, 814 tests passed**. Log: `artifacts/frontend-final-test.log`.
- Latest vector icon / account workspace layout changes: **3 affected files, 296 tests passed**. Log: `artifacts/layout-tests.log`. The final one-line flex propagation correction was checked in the rendered page and final production build.
- TypeScript and Vite production build: passed. Log: `artifacts/layout-build.log`.
- Native `cargo check --lib`: passed.
- Native full suite: **1938 unit tests + 4 plugin boundary tests passed**, 6 ignored, no failures. Log: `native/tests.log`.
- Final explicit export-path correction: **26 handler tests passed**, including exact filename preservation and avoiding unintended neighboring-file replacement. Log: `native/handlers-tests.log`.
- Windows native release build: passed; staged as `lib/Windows.dll`.
- Packaging behavior checks: **5 tests passed** (native library required, production manifest, archive digest, relative/missing assets and invalid entry paths).
- Final `dist-pack/otools-ai-switch-0.1.0.oplg`: **12 entries**, Windows x64 native library, CRC/SHA256/entry assets/production panel manifest verified.

## Real-library integration

`python -I scripts/smoke-native.py` loads the actual release DLL through its public FFI, always with a fresh temporary data directory. It verifies:

- Settings/database isolation, command boundaries, event polling and FFI response/free.
- Exact selected export path without silently changing the overwrite target.
- Fixed port 10888 and visible port-conflict failure.
- Missing/invalid local API keys rejected; platform-header bypass rejected.
- Codex login token plus explicit local key supported; invalid explicit key never falls back to another identity.
- Browser CORS preflight accepted while management/static/unrelated upstream API paths are rejected.
- A local mock upstream receives the mapped model and its own credential; JSON and SSE responses return correctly and usage is recorded.
- An active, held SSE connection is cancelled and shutdown completes within the host's three-second budget.
- Stop/start, retired-library late-call rejection, explicit initialization and persisted isolated key state.

No real AI accounts, paid upstream calls, user client configuration writes or original ai-switch database imports were used.

## GUI scope

Browser checks used the built frontend with a test-only OTools facade connected to the real native DLL and a temporary database. The tool is `scripts/preview-plugin.py` and is excluded from the install package. Checked account-pool loading, language switching, account form navigation, settings and image workspace, removal of the client-setup sidebar entry, vector SVG identity and account workspace geometry at the default 1200×800 size.

The final rendered account workspace fills the 800px panel: toolbar ends at y=60, scroll region and account section end at y=768, followed by the 32px status bar. The empty state stretches from y=164 to y=744 (580px high), with the list container using `flex: 1 1 0%`. The vector entry renders its own curve/anchor SVG, and no client-setup sidebar button is present.

This is not an actual OTools desktop installation or remote-host acceptance run. The SDK desktop close-event contract and remote host-file dialog behavior are covered by focused tests using the real vendored SDK implementation. An actual remote host/CLI, macOS/Linux native binaries, real OAuth, system certificate installation and real provider multimodal generation were not exercised here.

## Ignored upstream tests and warnings

Six Rust tests remain intentionally ignored because they require installed Codex CLI/OAuth, live provider credentials, explicit user database/session files or real-corpus benchmarks:

- `official_cli_browser_login_start_and_cancel_smoke`
- `live_agentrouter_brotli_workbuddy`
- `live_responses_encrypted_content_recovery`
- `probe_one_m_declaration`
- `verify_live_codex_response_id_matching`
- `report_real_usage_and_cache_speedup`

The retained OCRAD dependency emits legacy asm.js/eval and browser-externalized fs/path warnings; Vite also reports a large main bundle. Rust emits inherited unused/dead-code warnings. These are warnings, not passing claims for the untested real-account/platform paths.
