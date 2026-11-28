# Middleware responsibilities implementation plan

> **For agentic workers:** Use executing-plans inline; keep current main and existing uncommitted changes.

**Goal:** Implement approved three-plugin recovery design with transparent configurable matching and actual lifecycle placement.
**Architecture:** Version 2 pipeline separates recovery from request/response. Shared native effective-policy resolution and failure rule evaluation drive runtime and preview. Catalog owns descriptions/defaults; UI only shows installed instances.
**Tech Stack:** React/TypeScript, Rust, embedded QuickJS.
**Spec:** ../specs/2026-11-28-middleware-responsibilities-design.md (three independent plugins approved).

## Constraints
- Preserve prior compact-list work and unknown pack configuration.
- Disabled/removed plugins do not activate through defaults or legacy failure_policy.
- Migration is idempotent and preserves v1 actual behavior including passthrough.
- No paid/network upstream test calls; use fixtures/local tests.

## Review focus
- V1 default-disabled rows concealed effective native defaults.
- Response passthrough previously disabled request cleanup.
- HTTP 200 structured error versus ordinary response text.
- Stream error recovery must not duplicate emitted output or inherit an unrelated plugin budget.
- Warehouse account health must use same policy as proxy.

## Tasks
- [x] 1. Native v2 migration, three recovery plugins, catalog defaults/descriptions, centralized effective policy. Add migration and disabled/removal tests; run red then green.
- [x] 2. Shared failure-rule model/defaults/validation and evaluator. Wire retry and stream error decisions, explicit diagnostics/preview. Test status/code/message combinations and guardrails.
- [x] 3. Frontend v2 migration/defaults, lifecycle filtering, searchable add catalog, rule editor/preview and purpose descriptions. Test sparse defaults, CRUD persistence and matching configuration.
- [x] 4. Audit self-heal paths, disable semantics and account accounting; integrate meaningful regression tests.
- [x] 5. Run relevant native/frontend suites, typecheck/build, review diff; record results and remaining limits honestly.

## Verification completed

- Native final suite: 2152 passed, 0 failed, 6 ignored. `artifacts/middleware-native-final.log`.
- Frontend final suite: 90 files, 936 passed. `artifacts/middleware-frontend-final.log`.
- TypeScript: passed. `artifacts/middleware-typecheck-final.log`.
- Vite production build: passed; existing OCR eval and bundle-size warnings remain. `artifacts/middleware-build-final.log`.
- `git diff --check`: passed (Git emitted line-ending normalization notices only).
- Browser fixture: actual React editor and catalog at 341px editor width; collapsed and expanded layout measured without horizontal overflow. This checks the fixture UI, not installation into the desktop host.
- Independent native review found stream activation, quota failover, prelude disconnect, inherited stream budgets and independent health-accounting issues; all were corrected and covered by 7 passing v2 regression tests.
- One earlier native run hit an OS file-sharing violation in the pre-existing corrupt-database recovery test. The final full run passed, including that test.

## Delivery notes

- User approved three independently addable recovery plugins: retry, stream recovery and account health.
- New-account defaults install retry and account health; other built-ins are added on demand, with existing service-specific compatibility defaults retained.
- Explicit v2 missing/disabled recovery no longer inherits old policy defaults. Legacy migration retains effective settings and archives legacy schedule configuration in `legacy_plugins`.
- Legacy broad retry is labeled as compatibility behavior. Its displayed compatibility rules preview the explicit matching behavior that takes effect after editing or restoring rules.
- Rules use exact/range status matching, code/type matching, message contains/prefix matching, AND/OR groups and exclusions. Protocol-terminal constraints remain visible constraints, not arbitrary retry overrides.
- UI uses three lifecycle categories (request, response, recovery), with a searched catalog inside the selected category. No separate all-categories view was added.
- No release, push, installation or changes to real upstream accounts were performed. All implementation changes remain in the workspace for review.

## 2026-11-28 follow-up: error-oriented plugin introductions

- Updated catalog descriptions to start with recognizable error text or the user's observable problem.
- Stream recovery names `stream disconnected before completion` / `stream closed before response.completed`, explains both continuation switches and where recovery can succeed.
- Request encryption cleanup names `invalid_encrypted_content`, verification/decryption failures, and explains when to select removal of all encrypted content. An upstream saying encryption is unsupported is described as a scenario, not an invented universal error string.
- Azure cross-resource cleanup quotes the actual tested Azure error and explains why removing encryption alone cannot clear foreign resource IDs.
- Added concrete examples for temporary retry errors, missing reasoning (11155), content rejection and excessive image counts; other plugins describe observable symptoms without fabricated error codes.
- Expanded guidance now appears before configuration controls. Long error text wraps and preserves line breaks.
- Follow-up validation: 18 component/pipeline tests, 5 native catalog/policy tests, TypeScript and Vite build passed. Only descriptions and description presentation changed in this follow-up.
