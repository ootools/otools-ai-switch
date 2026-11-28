# Compact Pipeline Settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the wrapped account-processing flow canvas with a compact, accessible settings list.
**Architecture:** Retain the controlled PipelineFlowEditor public interface and PipelineConfig shape. Render one direction using tabs, with a local single-expanded-row state and immutable chain operations; leave persistence and backend untouched.
**Tech Stack:** React 18, TypeScript, UnoCSS, lucide-react, Vitest, Testing Library.
**Spec:** docs/superpowers/specs/2026-11-27-compact-pipeline-settings-design.md

## Global Constraints
- No new dependencies, backend changes, branch/worktree creation, automatic commits, or unrelated UI redesign.
- Preserve plugin configs (including unknown keys), chain order, disabled instances, passthrough chains, and both directions' state.
- Keep native select/number/textarea/bool inputs and existing component props/persistence shape.

## Review Focus
- Disabled existing plugin is re-enabled by Add without duplication or resetting config.
- Unknown plugin/config data survives adjacent edits, sorting, and direction changes.
- Switching to passthrough and back does not erase a direction's settings.
- Reordering/deleting the open item keeps details attached to the correct instance.
- Keyboard tabs and narrow/dark layouts remain usable without overflow.

### Task 1: Compact editor behavior (RED → GREEN)
**Files:** modify src/components/PipelineFlowEditor.tsx and src/lib/i18n.tsx; create tests/PipelineFlowEditor.test.tsx.
**Interfaces:** preserve Props { pipeline: PipelineConfig; catalog: BuiltinPluginDescriptor[]; onChange(pipeline): void }.
- [x] Write tests for direction tabs, one collapsed/expanded details row, field persistence, switch state, preserved unknown keys, add/re-enable, ordering/removal, passthrough, empty chains, and keyboard navigation.
- [x] Run pnpm exec vitest run tests/PipelineFlowEditor.test.tsx and observe expected feature failures.
- [x] Implement list, direction tabs, parameter summaries, switches and localized strings using existing controlled state patterns.
- [x] Re-run dedicated tests and pnpm typecheck; expect PASS.

### Task 2: Integration and visual verification
**Files:** modify tests/AccountsScreen.test.tsx and src/styles.css; inspect src/lib/useFlowGeometry.ts (keep unrelated code untouched).
**Interfaces:** Task 1's editor remains embedded in the existing account Request Processing tab.
- [x] Update the integration case to assert a single visible lane and direction switching; run it before implementation to establish RED.
- [x] Remove obsolete connector styles and finish scoped responsive/focus styling without changing global styles.
- [x] Run pnpm test:run, pnpm typecheck, and pnpm run build; record any unrelated baseline failures.
- [x] Render real editor fixtures at 375px and dialog width; verify interactions and light/dark computed layout. Screenshot visual inspection unavailable (see verification notes).
- [x] Review diff and record completion/verification results. Leave changes uncommitted for the user.
## Verification results (2026-11-28)
- Dedicated editor tests: 13 passed, including numeric/select/bool/textarea edits, opaque config preservation, disabled re-enable, unknown plugins, reordering/removal and keyboard focus.
- Full frontend suite after final patch: 88 files / 929 tests passed (175.96s).
- TypeScript: `node node_modules/typescript/bin/tsc --noEmit` passed.
- Production build: `node node_modules/vite/bin/vite.js build` passed. Existing OCR dependency/eval and chunk-size warnings remain.
- Local test runners were invoked through their Node entrypoints because the initial pnpm wrapper did not find vitest in the restricted execution environment. No dependency or package configuration changed.
- Browser: production editor with real native manifest; request/response switch and expansion checked; 447px editor and 375px container / 343px editor fit without horizontal overflow. Three expanded retry fields adapted to one column in the narrow fixture. Dark background/text computed correctly; browser console had no errors.
- Screenshot tool returned metadata without usable image pixels; native browser bridge unavailable, headless launch blocked by tool policy. Visual pixel review was not completed. Geometry/accessibility/interaction verification completed through the permitted browser tools.
- Independent read-only review: no important findings. `git diff --check` passed.
- Changes remain uncommitted. Preview fixture and logs are in ignored artifacts / .superpowers directories.