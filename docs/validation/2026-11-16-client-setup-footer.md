# Client setup bottom write bar — 2026-11-16

## Behavior

- The client setup page keeps the entire existing write row (scope hint and submit button) at the bottom of the main-content area, visible from the first frame and throughout scrolling.
- The heading, client list, outcome/error messages and manual configuration parameters share an independently scrolling body.
- The footer reserves its own space rather than covering the last client. Its height adapts to wrapped hint text in narrow windows.
- The ClientSetup route owns scrolling in AppLayout; other non-workspace screens retain their existing scroll behavior.
- The original form submission, client selection, disabled/pending guards and custom-path handler remain unchanged. Claude slot dialogs remain outside the new form/scroll body.
- No sidebar-width offsets, viewport-fixed overlays or new high-z-index layers were added.

## Validation

- Two new regressions failed before the change: the shell still scrolled the whole page, and no independent client setup scroll region existed.
- ClientSetupScreen and AppLayout tests: 48 passed.
- Complete frontend suite: 78 files / 917 tests passed.
- Typecheck and production build passed. Existing OCRAD/browser externalization/eval/chunk-size warnings remain.
- Packaging regressions: 5/5 passed.
- Browser display-only host fixture: `artifacts/client-setup-footer-gui/index.html`; no installed accounts, client files or upstream services were used.
- At 1440 × 1000, footer bounds remained top 940 / bottom 1000 before and after scrolling; the list ended above the footer, document scroll stayed zero, and the footer did not overlap the sidebar.
- Sidebar collapse retained the footer at the viewport bottom and within main content.
- At 420 × 720, footer bottom remained 720, hint and button fit within its 64px height, last client/manual disclosure were reachable, and no page horizontal overflow occurred.
- Focused independent source review found no important issues.

## Delivery scope

Updated local test package: `artifacts/client-setup-footer-package/otools-ai-switch-0.1.5.oplg` (17 entries; CRC/SHA256 and Windows native-library boundary verified). This is a local validation artifact, not a published release. Native code was unchanged and its tests/build were not rerun for this UI change. No commit, push, publication, installation or standalone sync was performed.
