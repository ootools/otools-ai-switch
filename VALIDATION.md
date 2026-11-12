# Validation record

Validated locally on Windows x64 with Node 22.22.2, pnpm 10.33.2, Rust 1.94.1 and Python 3.12.10.

## v0.1.2 release verification

- Package, frontend and native version are aligned at `0.1.2`.
- Frontend: **75 files, 861 tests passed** (`pnpm test:run --maxWorkers=4 --minWorkers=1`), typecheck and production build passed; packaging unit tests **5 passed**.
- Native: **1955 unit tests passed**, 6 intentionally ignored (`cargo test --lib -- --test-threads=1`). The Windows release DLL was rebuilt because the Rust retry logic changed.
- New regression coverage: `response.created → response.failed` retried before any client output; a `capacity` class configured for 100 retries while the global count is zero; an unrelated semantic failure unaffected by the capacity override; capacity exhaustion does not increment `transient_failure_count` or cool the account; failure-class word list does not misclassify context-length, quota or storage errors as capacity.
- AI listener default port moved from `10888` to `29527`; the port is not persisted in the database, so changing the native default is sufficient. Smoke, autostart and endpoint fixtures were migrated accordingly. Clients configured with the old address must be updated manually.
- Final rebuilt release DLL: **7 FFI / listener checks** (`smoke-native.py`), **3 background startup tests** (`test_autostart.py`) and **2 Nano Banana native integration tests** (`test_nano_banana.py`) passed with isolated storage and local mock upstreams.
- Release package: `dist-pack/release-0.1.2/otools-ai-switch-0.1.2.oplg`; verified 12 entries, CRC/SHA256/relative assets/native exports/panel manifest. SHA256: `4397b693ace167feab8eb438e8056eec1e095184989e10d6766d510c2b757bc0`.
- Platform packages: CI builds one native library per matrix job and verifies with `--expect-library` that each `<packid>-<version>-<platform>.oplg` contains exactly that single library; a deliberately wrong expectation was confirmed to be rejected locally. POSIX hosts needed `SO_REUSEADDR` in the autostart port-conflict test because earlier probes leave 29527 in `TIME_WAIT`.
- CI runner fix: pinning `macos-14` made GitHub resolve the job to the unsupported `macos-14-arm64-us-default` configuration, so the job was cancelled before any step ran (0 steps, no runner assigned, check-run annotation `The configuration ... is not supported`). Using the `macos-latest` alias (`macos-26-arm64`, already proven by the sibling `otools-idescriptor` workflow) restores the macOS build.
- All three platform packages were taken from the fully green CI run `43174071660` (Windows / Linux / macOS jobs each `success`, 26 steps). Published Release SHA256: Windows `97d90c289a8447efdadf6e2e371f572deacfb5b3dc62aa3f22da26947f8e6d0d`, Linux `f3e410c403814c47845b431c163fc0105f1623ecd9977dbdcfde767078a07da6`, macOS `7b168b907144e651c2b696b0937bc229a00a061b9dbf8ba6a66b43627ac67d83`; each archive verified locally to contain exactly one native library (`lib/Windows.dll` / `lib/Linux.so` / `lib/macOS.dylib`). The macOS dylib Mach-O header reports `cputype 0x0100000c` (ARM64). Rust Windows DLL rebuilds are not byte-reproducible, so an earlier successful run produced a different DLL hash (`3d51c4...`); the other 11 archive entries were byte-identical. The Release `SHA256SUMS` is the authoritative checksum list.
- Platform scope of the locally built package is Windows x64; Linux x64 and macOS ARM64 packages come from the CI matrix. Real account OAuth, paid generation and actual OTools desktop/CLI restart acceptance were not exercised. Older release sections below record earlier versions.

## v0.1.1 release verification

- Package, frontend and native version are aligned at `0.1.1`.
- Frontend: **75 files, 857 tests passed**, typecheck and production build passed. Logs: `artifacts/release-0.1.1-frontend-tests.log`, `artifacts/release-0.1.1-web-build.log`.
- Native: **1943 unit tests + 4 integration tests passed**, 6 intentionally ignored; Windows release build passed. Logs: `artifacts/release-0.1.1-native-tests.log`, `artifacts/release-0.1.1-native-build.log`.
- Final release DLL: real FFI/AI listener smoke, **3 background startup tests**, and **2 Nano Banana native integration tests** passed with temporary storage and local mock upstreams. Logs: `artifacts/release-0.1.1-smoke.log`, `artifacts/release-0.1.1-autostart.log`, `artifacts/release-0.1.1-nano-banana.log`.
- Release package: `dist-pack/release-0.1.1/otools-ai-switch-0.1.1.oplg`; archive/CRC/SHA256/relative assets/native symbols/panel settings verified. SHA256: `434186602a6be72c08e45933eb0e109099e5fb41756dafc06988412cd3bf4360`.
- All **39 historical SQL migrations** remain byte-identical to upstream. Source and regression-test directories are included; no build outputs, database files or detected credential material are in the source publication set.
- Platform scope is Windows x64. Real paid generation, user OAuth and actual OTools desktop/CLI restart acceptance were not exercised; the startup and model tests simulate the host's native invocations. Older update sections below record the earlier local-only stages, not additional releases.

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
- Fixed port 29527 and visible port-conflict failure.
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

## Nano Banana / Nano Banana Pro update

- Image presets now distinguish Nano Banana (`gemini-2.5-flash-image`) and Nano Banana Pro (`gemini-3-pro-image-preview`). Google presets sharing one Base URL are identified by their saved image model mapping, not URL order.
- Final frontend regression: **75 files, 857 tests passed**. Log: `artifacts/nano-banana-full-tests.log`. TypeScript and production build passed (`artifacts/nano-banana-build.log`).
- `python -I scripts/test_nano_banana.py`: **2 real-DLL integration tests passed**, covering both distinct image model entries and actual mapped `generateContent` requests, key placement, image response conversion and stored asset bytes. Tests use only a local mock upstream and temporary plugin data; no Google API calls, quota usage or real-account availability validation occurred. This update reuses the existing native Gemini image bridge; it does not add a new native implementation or require Pro imageConfig fields.
- Pro model ID was confirmed from Google’s official cookbook and developer announcement: https://raw.githubusercontent.com/google-gemini/cookbook/main/quickstarts/Get_Started_Nano_Banana.ipynb and https://blog.google/technology/developers/gemini-3-pro-image-developers/ . The direct REST documentation timed out during lookup; real Google endpoint compatibility and model access remain untested here.
- Local update package: `dist-pack/nano-banana-update/otools-ai-switch-0.1.0.oplg`. No commit, push or market publication is part of this update.

## Image-account configuration update

- Final frontend suite: **75 files, 854 tests passed**, no failures. Log: `artifacts/image-account-full-tests-final.log`. Typecheck and production build passed (`artifacts/image-account-build-final.log`).
- Final native full suite: **1943 unit tests + 4 integration tests passed**, 6 intentionally ignored. Log: `artifacts/image-native-full-tests.log`. Release DLL built successfully and staged as `lib/Windows.dll`.
- Image create/edit tests cover image-only protocol choices, OpenAI/GPT Image, Google Gemini and Seedream presets, their displayed upstream POST endpoints, automatic `image.generate` mappings and preservation of hidden existing settings.
- Content filtering, Brotli request compression, reasoning/tool compatibility and text-context controls are shown only in general accounts; image/video/audio create forms do not expose them.
- Native regressions reproduce and fix Ark `/api/v3/v1/images/generations` becoming `/api/v3/images/generations`, Seedream image parameters, GPT Image's unsupported `response_format`, and URL-image asset downloads without forwarding the generation credential. Downloads use the plugin outbound client, HTTP(S) only, bounded timeout/size and supported raster-image validation.
- Local mock tests validate these protocol and response contracts; no real OpenAI, Google or Seedream paid generation was performed. Google support here is Gemini `generateContent` image output, not Imagen `predict`, and GPT Image uses an API key rather than a ChatGPT login token.
- Browser verification used the real final DLL in temporary storage: image create options contain only the image protocols and presets, Seedream endpoint is correct, and advanced options no longer show text compression/filtering/reasoning. Prior default-Chinese, right-aligned filters and separated toolbar button groups remain included.
- Final native smoke and three background-start tests passed (`artifacts/image-native-smoke.log`, `artifacts/image-autostart-tests.log`).
- `dist-pack/image-account-update/otools-ai-switch-0.1.0.oplg` passed archive/asset/native/manifest verification. This is a local update; no commit, push, release overwrite or market publication was performed.

## Compute-pool and multimodal update

- Latest full frontend regression: **74 files, 839 tests passed**, no failures. Log: `artifacts/pool-media-full-tests-final.log`.
- TypeScript and production build passed. Log: `artifacts/pool-media-build.log`.
- The latest real release-library smoke passed account access, port conflicts, start/stop, JSON/SSE forwarding and shutdown. Log: `artifacts/pool-media-native-smoke.log`. No native implementation changes were needed for this update.
- Added native error summaries with code/details and retry for account reads and AI-service start/stop. The original user-reported installed-host failure was **not reproduced** in this environment; the current test facade and release DLL read accounts and start 29527 successfully. Simulation tests explicitly inject failures to verify UI recovery and do not count as reproducing that original failure.
- Image/video/audio endpoints and the shared masked key render inside one account-workspace shell. Text curl/config-write/generation-test controls and baseline settings are absent from non-general views. General baseline settings live inside the full-width compute-pool strip. Vector inference remains unavailable and no embeddings endpoint is advertised.
- Rendered image-page geometry at a 1200×700 test viewport: workspace height 700; toolbar 60; access panel approximately 129; list scroll region approximately 479; footer 32 fixed at y=668. The pool strip spans the full content line within its 12px side padding. Image Studio navigation is absent from the image TAB; the separate system Image Studio remains available.
- `dist-pack/pool-media-update/otools-ai-switch-0.1.0.oplg` verified: 12 entries, CRC/SHA256/relative assets/native library valid, original upstream logo, no custom panel size, and the `start_route_proxy` autostart task included. This local update was not committed, pushed or republished over the existing release.

## Host background-start verification

The plugin declares the desktop host's native startup task in `plugin.json`: `autostart.enabled=true`, `once=true`, command `start_route_proxy`. `once` is the host's per-main-window session deduplication, not a once-ever installation marker. Opening the plugin Panel is not part of this path.

`python -I scripts/test_autostart.py` passed three real-DLL tests with separate temporary-data processes: cold startup before any page, startup again after a process restart, repeated-task idempotency, and a clear failed result when 29527 is occupied. This simulates the native requests made by the desktop host runner; it does not claim an actual user's OTools process was restarted. The test is wired into the build workflow.

The CLI host currently lacks the desktop Tools startup-task runner; CLI autonomous startup was not added or verified. Installing/updating a plugin in a currently running desktop host refreshes the plugin list but does not rerun startup tasks until host restart; the page start switch remains available for immediate startup.

## Ignored upstream tests and warnings

Six Rust tests remain intentionally ignored because they require installed Codex CLI/OAuth, live provider credentials, explicit user database/session files or real-corpus benchmarks:

- `official_cli_browser_login_start_and_cancel_smoke`
- `live_agentrouter_brotli_workbuddy`
- `live_responses_encrypted_content_recovery`
- `probe_one_m_declaration`
- `verify_live_codex_response_id_matching`
- `report_real_usage_and_cache_speedup`

The retained OCRAD dependency emits legacy asm.js/eval and browser-externalized fs/path warnings; Vite also reports a large main bundle. Rust emits inherited unused/dead-code warnings. These are warnings, not passing claims for the untested real-account/platform paths.
