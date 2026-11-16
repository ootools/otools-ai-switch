# Single-screen live routing implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. The user approved the preview and requested the real change; implementation stays inline in this session.

**Goal:** Adapt the approved compact preview into real, full-height 900×600 live routing without scrolling the default 15-client/12-account board.

**Architecture:** Keep live/simulation data acquisition unchanged. Preserve all safe model rows, derive visible rows and explicit overflow counts, and render a measured three-column grid. Use a focused portaled dialog for complete overflow lists/help/error details.

**Tech Stack:** Existing React/TypeScript, CSS/SVG motion paths, ResizeObserver, Vitest/Testing Library; no new dependency/native change.

**Spec:** `docs/superpowers/specs/2026-11-16-single-screen-traffic-design.md`

## Global constraints

- Target `900×600`, expanded sidebar `216px`, all `15` registered clients and `12` accounts in one screen; no main-view horizontal/vertical scrolling.
- Do not truncate underlying activity/catalog or falsify hidden account identity. Show explicit hidden active-node counts; selecting hidden nodes reveals them.
- Keep real/simulation isolation, safe data APIs, old unknown origin semantics, official-logo correction, 1.8-second visible simulation updates and 3-second snapshot reconciliation.
- Keep real gateway state, compact simulation notice, static idle paths, active-only dots and reduced motion.
- No new dependencies, native edits, account/config writes, version bump, commit/push/release or installation.

## Review focus

- More than 12 busy accounts: every row/attempt remains in model and dialog, total counts remain global, hidden busy number explicit.
- Same-size reorder or sidebar animation: links remain anchored to visible nodes in board-local coordinates; hover transforms must not drift sockets.
- Selection changes under live updates: deleted/idle selected rows clear or retain valid identity without jumping to a different row.
- Dialog trigger disappears during a live update: focus restores to stable help button fallback; Escape and Tab stay scoped.
- No host/failed snapshot in simulation: demo still works; real errors/status remain truthful and do not expand layout.

---

### Task 1: Retain complete safe model and bound board display

**Files:** Modify `src/lib/trafficState.ts`; create `src/lib/trafficLayout.ts`; tests `tests/lib/trafficState.test.ts`, `tests/lib/trafficLayout.test.ts`.

**Interfaces:** `buildTrafficModel` additionally returns `all_accounts` (all sorted accounts with `active_count`), retaining existing `accounts`. `buildTrafficViewport(model, {clientLimit, accountLimit}, selection)` returns visible clients/accounts, visible edges, hidden counts/busy counts. `TrafficSelection = {kind:"client"|"account";key:string}|null`. `trafficRowLimit(height:number, max:number):number` uses min row22px, gap4px, title27px, bottom padding18px, defaults15clients/12accounts when no measurement. Pin a valid selected hidden row by replacing the last visible row; never add an anonymous edge.

- [ ] RED model tests: 14 idle members stay in `all_accounts`, main `accounts` remains12; active outside-pool metadata stays intact.
- [ ] RED viewport tests: 15 clients/12 accounts visible; 14 busy accounts reports hidden2busy; retained globaltotal14; selected14th appears with its real edge and one displaced row; no hiddenedge is fabricated; height520 fits15/12, height300 returns usable smallerlimit.
- [ ] Implement these exact interfaces without altering snapshot/event/count code.
- [ ] GREEN: `pnpm test:run tests/lib/trafficState.test.ts tests/lib/trafficLayout.test.ts`.

### Task 2: Compact interactive real page and accessible overflow

**Files:** Modify `src/screens/TrafficScreen.tsx`, `.css`, `src/components/layout/AppLayout.tsx`, `src/lib/i18n.tsx`; create `src/components/traffic/TrafficDialog.tsx`, `src/lib/useTrafficGeometry.ts`; tests `tests/TrafficScreen.test.tsx`, `tests/AppLayout.test.tsx`, `tests/TrafficDialog.test.tsx`, `tests/lib/useTrafficGeometry.test.tsx`.

**Interfaces:** TrafficScreen existing props unchanged. `TrafficDialog({title,onClose,returnFocusRef,children})`, portaled body, focusClose/Tabtrap/Escape/backdrop/connected opener restoration. `useTrafficGeometry(clientIds:string[],accountIds:string[])` returns boardRef/hubRef/clientRefs/accountRefs, dimensions and node paths, measured/limits; use layout commit+RO+rAF, transform-normalized boardcoordinates, cancel scheduledframe/observer ondispose. Default fallback dimensions allow jsdom rendering but never assume screen dimensions in production.

- [ ] RED UI: Traffic shell owns `overflow-hidden`, no old minimum860 width/tall inlineheight, 12 accountmain rows, `View all 14 accounts` modal includes all14; busyoverflow explicitly2active. Hidden rowselection returns actualrow+edge.
- [ ] RED UI: clicking activeclient highlights only itsedges; clear/Escape resets; simulation toggle never forgesgatewaystatus and stopreturnslatestrealdata. Preserve existing simulation/resync/error tests.
- [ ] RED dialog tests: portalsoutsideboard, Closeinitialfocus, Tab/ShiftTabwrap, Escapecleanup, openerrestoration/disappearedtriggerfallback.
- [ ] RED geometry tests: nonuniform/scaledbounds normalized to localpx; same-sizeIDorderchange recomputesanchors; resize scheduledframe cancelled onunmount; missingnode has no invented path.
- [ ] Implement fullheight compactheader+board+footer. Header includesreturn,simswitch,refresh,help. Grid3cols min-width0; fixedmaxnodeheights~32px forfewrows, distributedrowsforfull15, centered90pxhub, no buttontransforms. More controls/truthfulerrors/helpdialogs, text/titlefullnames. Longsimulationhintvisuallycompact but accessible.
- [ ] Add bilingual help,overflow,hiddenbusy,select/clearemphasis and errorlabels; move legacyhint/read-only explanation intohelp. Realstatussyncpending neverreportedzeroactivity.
- [ ] GREEN: `pnpm test:run tests/TrafficScreen.test.tsx tests/AppLayout.test.tsx tests/TrafficDialog.test.tsx tests/lib/useTrafficGeometry.test.tsx` then typecheck.

### Task 3: Real rendered acceptance and documentation

**Files:** Existing isolated trafficGUI fixture plus new compact-production fixture under ignoredartifacts; README and `docs/validation/2026-11-16-single-screen-traffic.md`.

- [ ] At900×600 defaultsidebar216, measure15clients/12accountswithinbounds and noouteroverflow. Check anchors/dots andofficialimageloading.
- [ ] Verify sidebarcollapse/resize to320, 900×480, en/zh,dark/reducedmotion, activeaccount>12modal andselectedhiddenidentity, closedialogfocus.
- [ ] Run `pnpm test:run`, `pnpm typecheck`, `pnpm build`, `node --test scripts/package.test.mjs`; create/verify a new isolatedlocalpackage, not a publicrelease.
- [ ] Independentfocusedreview and fixactionablefindings. Update currentREADME/validation faithfully without alteringhistoricalreleasefacts.
- [ ] Final report actualresults and state noautomaticcommit/push/publication/installation.
