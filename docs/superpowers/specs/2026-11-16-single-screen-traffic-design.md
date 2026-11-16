# Single-screen live routing design — 2026-11-16

## Approved direction

The user approved adapting the independent `artifacts/traffic-compact-gui` preview into the real page after changing the target to 900×600 and requesting no main-view scrolling. Keep a compact three-column monitor: clients → small gateway → accounts. This supersedes the earlier tall canvas and its horizontal scrolling, not the live activity/authentication design.

## Goals and layout

- At a 900×600 app viewport with the normal 216px sidebar expanded, show all 15 registered clients, 12 accounts, gateway and controls within one content screen, without horizontal or vertical main-view scrolling.
- One short header containing title, return, simulate/stop, refresh and explanation. Do not add summary cards, a permanent subtitle or long footnote.
- Height-constrained board with regular single-line node rows; 20px client artwork, approximately 11px names, a roughly 88×120px gateway. Keep real gateway status even in simulation.
- Active-first sorting and all-client/all-account totals remain. Use explicit more controls for nodes beyond the board capacity. Extra active-node counts must be visible; never silently discard active requests or connect hidden identities to an unlabeled aggregate endpoint.
- More dialogs contain complete, safe current-pool/active-outside-pool data; selecting a hidden node reveals that node in the board and closes the dialog. Only a dialog may scroll.
- Size the visible client rows from the measured board height with a minimum usable 22px row and a maximum of 15 default rows. Accounts default to 12; smaller windows may reduce visible rows with explicit more controls rather than shrinking them below the minimum. The data model retains everything.
- Permanent idle dashed paths for displayed nodes, colored moving overlays only for confirmed real activity or explicitly labeled local simulation. Clicking a node emphasizes its corresponding live edges; same click / clear / Escape clears emphasis.
- Explanation/error details move into a small accessible dialog. A compact error indication remains visible without allowing a long message to grow the board.

## Architecture and boundaries

- Extend `buildTrafficModel` with `all_accounts` while preserving its existing `accounts` result and all attempt/request counting semantics.
- Put display slicing/hidden counts/pinned selection in a small pure `trafficLayout` helper; selection never mutates the snapshot or real catalog.
- Measure DOM node/hub anchors in a shared board coordinate system with transform normalization; ResizeObserver plus a frame-coalesced measurement handles sidebar changes, node reordering and board-height changes. Do not scroll node columns.
- The Traffic route owns a bounded full-height shell; other screens retain their existing scrolling. Keep overlays outside the board and above the existing page stacking context.
- Use a focused dialog wrapper that traps Tab, handles Escape/backdrop close, restores focus, and confines scrolling to its content.
- Preserve `useTrafficActivity` snapshot/event reconciliation, real/simulation isolation, safe metadata APIs, existing official-logo corrections and reduced-motion support. No native changes, new dependencies, account/config writes or publication.

## Alternatives

A scaled version of the previous fixed canvas makes labels too small and still wastes the gateway area. Independent scrolling side lists hide the very relationships being monitored and complicate correct SVG anchors. The approved fixed-height board plus explicit overflow dialogs keeps the normal 15-client/12-account view complete and makes exceptional overflow explicit.

## Acceptance

Automated coverage for all-account preservation, hidden active counts, pinned hidden nodes, selection clearing, explanation/error dialogs, keyboard/focus restoration and simulation transitions. Browser measurements at 900×600 expanded/collapsed sidebar prove all default nodes fit, paths hit the real sockets, official images load, and the main content has no scroll overflow. Also check 900×480 and a narrower/320px-sidebar case for explicit overflow controls rather than clipping. Run the full frontend suite, typecheck/build and local package verification; do not commit/push/release unless separately requested.
