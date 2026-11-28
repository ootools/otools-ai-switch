# Middleware architecture review fixes

**Goal:** Fix all ten verified architecture review findings without losing earlier work.
**Execution:** executing-plans inline, test-driven where behavior changes, current main only.
**Scope:** lane isolation, continuation attempt parity, account settlement ordering, coherent reloads, image I/O concurrency and SSRF/size bounds, one failure decision per event, unavailable instance persistence, atomic install, full header propagation.

## Tasks
- [x] A. Unified runtime bundle and per-plugin execution locks; serialized generation-checked publication; pinned, bounded image fetch; installation staging/rollback.
- [x] B. Select only the correct direction, preserve no-op bytes, accept retained unavailable/disabled entries, propagate stage context and all headers.
- [x] C. Share outbound attempt and response-header preparation with continuation; record failure decisions without re-invoking hooks.
- [x] D. Apply explicit plugin account effects after host settlement using per-attempt queues; verify cooldown and failure ownership.
- [x] E. Run relevant and full native/frontend tests, typecheck/build; independent final review and documentation update.

## Required regression cases
- Dual-hook plugin in one lane cannot run another lane; two lanes cause one call per appropriate stage.
- Reload started before disable cannot republish old generation; callers use coherent descriptor/runtime snapshots.
- Slow image fetch cannot block unrelated plugins; DNS approved addresses pinned, mixed/private/mapped IPv6 rejected, chunked body bounded.
- New version with oversized/unloadable script cannot corrupt existing package.
- Disabled/missing plugin remains saved while unrelated account fields change.
- All attempts including continuation run same stage/header pipeline.
- Failure hook executed once for decision/logging; explicit cooldown survives quota/permission settlement.


## Final verification

- Native full suite: **2171 passed, 0 failed, 6 ignored**; `artifacts/review-final-native.log`.
- Frontend full suite: **90 files / 937 passed**; `artifacts/review-full-frontend.log`.
- TypeScript passed; `artifacts/review-typecheck.log`.
- Production build passed; `artifacts/review-build.log`. Existing OCR/eval and bundle-size warnings remain.
- `git diff --check` passed; only Git line-ending normalization notices.
- Focused review filter: 46 passed (includes 12 newly added review-specific cases and existing tests whose names contain preview/review).

## Fix mapping

1. Request/response body dispatch selects the matching lane; lifecycle response hooks select response/recovery and deduplicate IDs. No-op/failed body hooks preserve exact bytes.
2. Primary and continuation attempts share input stages, post-build reasoning stage, wire preparation and response-header hooks. A real streaming upstream test asserts all request stage headers/body marks on both calls and two response-header callbacks.
3. Per-attempt task-local failure event queues flush only after host settlement; stream completion uses the same ordering. A quota response integration test confirms explicit plugin cooldown survives and state records exactly one failure decision.
4. Descriptor/runtime objects publish as one snapshot with generation checks and a serialized build lock. Lazy reload rechecks freshness under the lock. Account events bind context filtering and invocation to one version.
5. Per-plugin runtime/engine locks remove global hook contention. Multithread Tokio uses block_in_place before blocking engine/build locks; a two-worker timer test verifies other tasks continue.
6. Image DNS+fetch shares one deadline; all resolved addresses must pass public-IP rules, including mapped IPv4 handling. Reqwest connects to pinned approved addresses, disables proxy/redirect, and chunks are bounded before accumulation beyond 16MiB.
7. Metadata generation no longer invokes failure hooks. Decision diagnostics are emitted from the actual decision invocation; integration test proves one invocation.
8. Retained unavailable pack IDs can remain in saved pipelines, including disabled instances, while execution skips unavailable packs.
9. Script validation and size checks precede mutation. Complete staged package and settings publish under loader snapshot lock. Failure restores old package/settings; exceptional rollback preserves backup. Uninstall invalidates even when settings persistence fails.
10. All request stages apply both body and header results via the shared helper, including retryRecovery; stage context contains the actual stage.

## Independent review follow-up

The final reviewer found four additional issues (Tokio starvation, uninstall failure invalidation, mixed descriptor/runtime generation for account context, repeated same-generation lazy rebuild). These were fixed and regression-tested before the final native run.

## Limits and delivery

- Runtime generation, transaction and failed-install tests are local/fixture based; no real accounts or paid upstream APIs were used.
- Filesystem staging provides rollback for reported I/O failures; there is no claim of multi-file power-loss atomicity.
- Already running plugin calls may finish; uncommitted persistent actions recheck current capability/account selection before writing. Committed changes are not undone by subsequent disable.
- Synchronous SDK uses block_in_place on the production multithread Tokio runtime; the synchronous API is not promised to keep timers responsive in a single-thread executor.
- No commits, release, push or desktop-host installation performed. Earlier uncommitted work is retained.
