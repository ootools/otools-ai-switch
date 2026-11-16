# Single-screen live routing validation — 2026-11-16

## Implemented

The approved compact preview is now implemented in the production TrafficScreen. It uses a bounded full-height header/board/footer, readable single-line client/account nodes and a small gateway instead of a fixed 860px-wide canvas whose height grew with account count. The Traffic route owns its layout overflow; other screens retain their scrolling.

- Default board: 15 clients / 12 accounts; smaller heights reduce only displayed rows with explicit more controls.
- Complete safe account summaries and actual activity remain in the model. More dialogs include all current-pool and active non-pool accounts. Hidden busy counts are explicit; selecting a hidden node reveals that exact node and its real edge.
- Static idle paths persist; dots/highlights require confirmed live activity or explicit local simulation. Real gateway status remains separate from simulation.
- Click to emphasize relationships, clear or Escape to reset. Explanation and error details live in accessible, portaled dialogs with focus management and Tab/Escape handling.
- Geometry normalizes screen bounds to board-local coordinates; reorder/resize and actual node/list sizes trigger measurement. Hover/press transforms are disabled on routing nodes.
- Prior uncommitted official WorkBuddy/CodeBuddy/ZCode/Qoder logo corrections are preserved.

## Automated verification

- Final complete frontend suite: **81 files / 949 tests passed**.
- Focused compact/model/modal/geometry/navigation suite: **58 tests passed**.
- Typecheck and production build passed; packaging regressions **5/5 passed**.
- Two independent-review P2 findings were reproduced and fixed: busy overflow footers changing row positions without resize, and a vanished non-pool selection resurrecting on the next request. Both regressions failed before the fixes and passed afterward. The geometry regression also covers capped-height nodes by observing their list containers.
- Existing OCRAD/browser externalization/eval/chunk-size and unrelated React test warnings remain. Native code was unchanged and not rerun for this UI change.

## Actual rendered checks

Test-only host fixture: `artifacts/traffic-single-screen-gui/index.html`, using the real production TrafficScreen, AppLayout and MotionPage. It never loads installed credentials or sends upstream/config-write operations.

- **900×600**, sidebar216px: all15clients/12accounts/gateway/controls fit the content; no main horizontal/vertical overflow. All local images load,27baseline paths remain, simulation gives12dots. Maximum measured visible-node socket error was below1px.
- **900×480**: the stable view fits13client rows and12accounts, with explicit＋2clients; minimum client row approximately23.74px. No main horizontal/vertical overflow.
- **14 busy accounts**: global attempt count14, main account rows12, hiddenbusy2. Overflow dialog lists all14; selecting OpenRouter备用 reveals its exact `zcode:account-13` edge and keeps the board bounded.
- **English at900×600 with sidebar320px**: content556px wide;15clients/12accounts remain, no main/header/footer overflow. Some long labels are ellipsized with full-name titles.
- The IAB was briefly hidden during checks, which pauses animation frames and MotionPage entrance; final acceptance measurements were taken after the pane became visible. Hidden interim geometry is not acceptance evidence.

## Scope and rulings

Updated isolated local package: `artifacts/traffic-single-screen-package/otools-ai-switch-0.1.6.oplg` (**23 entries**, CRC/SHA256 and Windows-native boundary verified). This is a development artifact, not a modification of public v0.1.6.

Unlimited nodes cannot all remain readable in one fixed screen: the main display is capped, complete data stays in named dialogs, and hidden busy counts/reveal actions keep exceptional activity accessible. Inherited status-query errors retain a visible error control and details; cached gateway status may remain until refresh/reconciliation. No other review issues were deferred.

No commit, push, public release, native rebuild, local installation or account/config/data migration was performed.
