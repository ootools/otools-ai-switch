# Uniform plugin host implementation plan

> Execute inline with executing-plans; preserve the current main work and existing changes.

**Goal:** Built-in and installed middleware use one descriptor, lifecycle dispatcher, SDK and scoped host action contract.
**Architecture:** Versioned descriptor metadata defines stages, capabilities and editors. QuickJS hooks return validated actions; shared executors own network/stream mechanics and account transactions. Built-in scripts decide their policies and error rules, without ID dispatch in production paths.
**Spec:** ../specs/2026-11-28-uniform-plugin-host-contract.md

## Constraints
- User approved implementation; no extra approval step.
- Do not reset earlier UI/copy/migration work or create a branch/worktree.
- Same capability names, context, default merging and action validation for builtins and packs.
- Fail closed for invalid actions; preserve original payload on hook failure; no partial hook effects.
- Compatibility migration can recognize old IDs; active executors cannot special-case business plugin IDs.

## Tasks
- [x] 1. Add shared manifest descriptor (API version, stages/hooks, capabilities, config editors) and real installed-pack resolution. Red/green test random pack with config and stage defaults.
- [x] 2. Shared JS SDK, lifecycle dispatch, policy domains, structured actions, diagnostics and validation. Red/green exception/invalid-action and random-pack tests.
- [x] 3. Wire retry/stream/health through generic events and actions. Migrate built-in scripts; remove ID checks; test random-ID retry, stream and account effects.
- [x] 4. Scoped account/state executor with transactions, idempotency, atomic counters and user paused/revoked protection; test actual DB actions.
- [x] 5. UI derives stage/editor/config from descriptor for external packs; test identical editing and adding behavior.
- [x] 6. Remove remaining request compatibility special IDs through shared stage/recovery contract; write SDK author docs, run full validation and independent final review.

## Review focus
- Installed pack IDs must match dispatch keys including pack: prefix.
- Missing/disabled pack cannot inherit built-in privileges or leave committed actions.
- Async work must not execute inside global JS lock.
- Stream event mutation only before output, with shared continuation budget.
- Repeated events and concurrent failures cannot duplicate account writes or overwrite paused/revoked state.


## Final validation

- Native full suite: 2159 passed, 0 failed, 6 ignored. `artifacts/uniform-final-native.log`.
- Frontend full suite: 90 files, 937 passed. `artifacts/uniform-full-frontend.log`.
- TypeScript and Vite production build passed. `artifacts/uniform-typecheck.log`, `artifacts/uniform-build.log`; existing OCR eval/bundle-size warnings remain.
- Installed random-ID copies of retry, stream recovery and account health exercised the production proxy and database paths successfully.
- Ten focused runtime tests cover installed ID/stage/defaults, isolated JS state, compositional stream transforms, invalid/throwing hook rollback, policy upper bounds, scoped account writes, duplicate event suppression, paused-state protection and removal-before-write.
- Native migration is LF-only and registered in migration_checksums.txt; existing migration bytes were not changed.
- Independent review identified duplicate failure accounting, stream composition, policy bounds, stop-rule fallback, disable races and unstable event identities. Corrections: account.failure is settled by the common repository exactly once in account_failure phase (not repeated in account_event); transforms compose; returned policies validated; stop is explicit; invalidation after persistence and lifecycle transaction ordering; stable process/request/attempt event identity.

## Implementation notes and decisions

- SDK API v1 uses deferred structured actions. Phase/capability restrictions are identical for builtin and external scripts. Direct account updates and namespaced state use the generic transactional action executor; account.failure produces the shared model/account settlement parameters and does not perform a second SQL increment.
- JSON request/response, response headers, SSE events, failure phases, completion and policy domains are dispatched through public metadata. Non-JSON bodies are not passed to JSON body hooks.
- Existing stage names remain public scheduling points for compatibility; any plugin can declare them. Active proxy/health/UI execution does not branch on business plugin IDs. IDs remain in registration, defaults, migration and test fixtures.
- Built-in compatibility switches now use stages or shared request.replace recovery. Legacy pre-pipeline fallbacks and required protocol bridge conversions remain host mechanisms, not private plugin capabilities.
- Persistent state is scoped by account + plugin, effect receipts are kept for the latest 4096 account events, diagnostics retain the latest 256 entries. No arbitrary SQL or unrestricted network API is exposed.
- Already committed actions are not undone by later disable. Global disable and writes serialize; account removal is rechecked under the write transaction.
- API author guide: `docs/plugins/middleware-api.md`; installable copy: `examples/middleware/account-health`.
- All changes remain uncommitted in the current workspace; no publish/push/live-account changes performed.
