# Live routing validation — 2026-11-16

## Implemented behavior

- Client configs carry `<platform-base-key>.<registered-client-key>`. Database keys and rotated aliases stay as original base keys.
- Legacy unsuffixed keys remain valid; their origin is displayed as unknown. No automatic rewrite of installed client config files was performed.
- Production AI router and proxy recognize registered suffixes; unknown suffixes remain invalid. Client attribution does not grant additional authorization or independent client revocation.
- Usage metadata and live logs record `client_key`.
- In-flight upstream attempts are stored in a shared registry with runtime epoch, sequence, snapshot, and explicit start/end transitions. Stream drop, retry and continuation switching are covered.
- Traffic entry is available from the general account-pool top toolbar and sidebar.
- Every displayed client and account retains a faint static dashed connection to the gateway, including while idle. Actual request paths add client-colored moving dots; completed requests remove dots/highlights without removing the base paths.
- Accounts are ranked by active attempts; idle current-pool members fill the view to twelve when available. All active accounts remain visible if more than twelve are busy.
- Traffic catalog uses a minimal account-summary API plus a pool-member-ID API rather than loading credentials or history.

## Automated validation

| Check | Result |
|---|---|
| Frontend typecheck | `tsc --noEmit` passed |
| Frontend full regression suite | 78 files, 919 tests passed |
| Native full unit suite | 2009 passed, 0 failed, 6 existing environment-dependent tests ignored |
| Frontend production build | Passed |
| Native release build | Passed, Windows native library staged locally |
| Packaging regression tests | 5/5 passed |
| Isolated local package CRC/SHA256/native boundary verification | Passed |
| Actual release FFI read-only smoke | Passed with temporary DB and six test account summaries; no secret/config data in response |

The existing native tests previously competed for fixed port 29527. Test-only runtime listeners now use ephemeral ports, retaining an explicit default-port conflict test; production behavior stays fixed at 29527. The installed OTools service was not stopped for validation.

Pre-existing build warnings remain (native unused code/imports and OCRAD browser externalization/eval/chunk-size warnings). Successful build output does not imply that those warnings were removed.

## GUI validation

Test-only preview: `artifacts/traffic-gui/index.html`; it supplies simulated host data, not installed accounts or upstream services.

- Idle topology: 15 clients, six of eight pool accounts shown, 21 static dashed paths; zero moving dots.
- Active simulation: ZCode 3 / WorkBuddy 2 / Claude Code 1 logical requests; account concurrency ordered 3 / 2 / 1.
- 12 dots in three distinct client colors moved along paths (`offsetDistance` changed across animation frames).
- Stopping simulation: active paths = 0, dots = 0, idle paths = 21.
- Light and dark layouts checked at a wide viewport.
- Earlier 620px viewport check: the then-present metrics used two columns and the graph scrolled horizontally (860px graph content), with no broken local images. The metrics were subsequently removed as recorded below.

## Page-local simulated traffic

The production Traffic toolbar provides **Simulate traffic / Stop simulation**. Simulation is page-local: it uses separate display data, varies account selection and concurrency every 1.8 seconds while visible, and never submits gateway/upstream requests or writes logs/usage. A clearly labeled Simulation mode notice and simulated-request legend prevent confusion with live data. The real gateway status remains visible and real activity continues updating independently; stopping immediately restores the latest real activity. No simulation setting persists across page mounts.

When no native catalog or accounts are available, the display supplies three demo clients and six clearly named demo accounts. Tests cover service-stopped demonstration, live/simulated isolation, dynamic routes, timer cleanup, and host-unavailable fallback. All 16 Traffic screen tests passed, as did the final 914-test frontend suite and typecheck; the frontend and isolated package were rebuilt and verified. Focused independent review found no important issues in the simulation addition. Native code was unchanged by this addition, so its prior 2009-test run remains the native verification record.

## Top statistics removal

The four summary cards above the visualization were removed at the user's request. Their dedicated styles, unused active-client count and translations were also removed. The page still provides its heading and toolbar, simulation notice, error messages, three-column topology, per-node counts and real gateway status.

A regression failed while the cards were present, then passed after removal. All 17 Traffic screen tests and the full frontend suite (78 files / 915 tests), typecheck, production build and five packaging tests passed. Browser inspection at 1440px confirmed that the topology now follows the heading directly, with node counts and gateway status visible. Existing OCRAD/build and unrelated React test warnings remain; native code was not changed or rerun.

The current isolated local test package is `artifacts/traffic-package-no-metrics/otools-ai-switch-0.1.5.oplg` (17 entries, CRC/SHA256 and Windows native boundary verified). The previous package remains unchanged. No commit, remote push, publication or installation was performed.

## Twelve-account minimum

The right-side display now fills to twelve accounts when enough current-pool members exist. Busy accounts still rank first by real concurrent attempts; every busy account remains displayed if more than twelve are active. Pools below twelve display only actual available accounts. Page-local simulation shares the same minimum and supplies named demo accounts to reach twelve.

Regressions failed against the previous six-account minimum, then passed after the change. Traffic state/UI tests: 26 passed; complete frontend suite: 78 files / 919 tests passed. Typecheck, production build and five package regressions passed. The updated local package is `artifacts/traffic-twelve-package/otools-ai-switch-0.1.5.oplg`, with 17 entries and CRC/SHA256/Windows native boundary verification.

Browser checks at 1440 × 1100 showed twelve right-side accounts and fifteen clients without overlap; twenty-seven static baseline links remained. Production simulation also retained twelve accounts and twenty-seven baseline links while showing twelve moving dots. The display-only fixture uses fourteen local fake pool members, not installed data. Native code was unchanged and not rerun; no commit, push, publication or installation occurred.

## Logos and delivery scope

New official project artwork is bundled locally for DeepSeek Harness website/desktop and Hermes Agent, with exact upstream MIT notices. Existing platform SVGs are reused for native clients. ZCode, WorkBuddy/AI, CodeBuddy and Qoder variants retain monogram fallbacks because redistribution/mark permission was not established; see `src/components/brand/agent-logo-sources.md`.

Local test package: `artifacts/traffic-package/otools-ai-switch-0.1.5.oplg`. This is a development validation artifact, not a newly versioned/published release. No market publication, GitHub release, installation, remote push, paid upstream request, or standalone `open/ai-switch` sync occurred in this implementation phase.

## Independent review and fixes

The independent source review found two important correctness issues: rapid runtime restarts could pin reconciliation to an obsolete event epoch, and detached pre-stop requests could repopulate a cleared activity registry. Both were reproduced by regressions and fixed: authoritative snapshots are no longer pinned to an event's epoch; stopping advances the service generation, cancels pending external requests/stream bodies, and rejects old-generation attempts.

Two account-catalog findings were also fixed: active non-pool accounts now retain their known summary values, and returning to Traffic refreshes current pool membership instead of using a fresh stale catalog. Regression tests failed before each correction and passed afterward. The final full suites and rebuilt artifacts above were run after these corrections. No deferred review findings remain.
