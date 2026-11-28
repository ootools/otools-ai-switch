# Request headers and image middleware consolidation

**Goal:** Move account User-Agent/custom headers into request middleware and consolidate remote image inlining + historical image count limiting. HTTP body Brotli compression remains independent.
**Approach:** Common script hooks and descriptor-driven editors; a catalog revision marks one-time migration of existing v1/v2/flat account settings. Keep unknown fields and legacy plugin configs. No new bitmap compression is implied or added.

- [x] Tests for header migration/removal, merged image switch combinations and scripts.
- [x] New requestHeaders/imageProcessing scripts and catalog; generic header table and UA preset field.
- [x] Native/TS migration parity, header execution through shared stages, remove duplicate advanced edit UI.
- [x] Account UI/creation compatibility; focused/full validation and final review.

Header precedence: migrated custom headers fill missing before provider auth; User-Agent and explicit replace/remove operations apply after normal identity construction. Provider auth and transport framing fields cannot be overridden by custom final header rules. Default image processing leaves image-count trimming off; enabling the trim option retains last 10 images unless configured otherwise. Remote inline uses existing bounded host image capability.


## Verification

- Native full suite: 2176 passed, 0 failed, 6 ignored (`artifacts/header-image-verified-native.log`).
- Frontend full suite: 90 files / 943 passed (`artifacts/header-image-verified-frontend.log`).
- TypeScript and Vite production build passed (`artifacts/header-image-typecheck.log`, `artifacts/header-image-build.log`). Existing OCR/eval and bundle-size warnings remain.
- Migration tests observed red→green for merged image toggles, header conversion, retained old configs, no resurrection after removal, omitted lanes, legacy passthrough behavior and mixed old/new entries.
- Native script tests verify set/remove behavior and protected headers, independent image trimming option and configured max count; runtime tests verify removed header plugin ignores stale config.headers.
- UI tests verify migrated UA editing/saving and external descriptor access to both new field editors.
- Independent review identified omitted-lane resurrection, Content-Encoding collision and duplicate retired IDs. All were fixed, with regression tests, before the final runs.
- No new pixel/image compression added. Existing HTTP body compression remains separate, with corrected name and purpose.
- Create form User-Agent is a convenience input in Request Processing that persists through the same native plugin migration; advanced edit no longer contains its own UA control.
- No commit, push, release or desktop installation performed. All prior workspace changes retained.
