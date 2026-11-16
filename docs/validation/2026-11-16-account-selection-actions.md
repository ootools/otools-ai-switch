# Account selection action strip — 2026-11-16

## Behavior

- Removed the account-list header select-all checkbox and its indeterminate-state handling; individual account selection checkboxes remain.
- The existing selection operation strip provides three buttons in order: **全选 / 反选 / 取消选择**. Its original visibility mechanism is preserved: at least one account must be selected before the strip appears; clearing the selection hides the entire strip immediately.
- Select all adds the current page's filtered account IDs without toggling already-selected accounts off. Invert changes only current-page IDs; selections on other pages remain. Clear removes all selected account IDs.
- Select all/invert are disabled for empty, loading, errored or placeholder page data. The entire strip, including batch status/move/delete controls, is absent when no accounts are selected.
- Existing filtering, pagination, group-switch and batch-operation selection lifecycle remains unchanged.

## Verification

- New list/card regressions failed while the header checkbox remained. The corrected pagination fixture then failed because the new selection buttons were absent.
- Focused selection/batch tests: **6 passed**.
- Full frontend suite: **81 files / 951 tests passed**.
- Typecheck and production build passed; packaging regressions **5/5 passed**. Existing OCRAD build/test warnings remain.
- Independent narrowly scoped review found no important issues.
- Built frontend with a real native-library test facade and temporary data: three local demo accounts only, no installed data, configuration writes or upstream requests. At900×600 and420×600 the selected-state buttons remain in the requested order on one row without operation-strip horizontal overflow. The corrected visibility mechanism was checked again at420×600: no strip initially, selecting one account shows it, and clear removes the strip and its batch controls immediately.
- Local validation package: `artifacts/account-selection-conditional-package/otools-ai-switch-0.1.7.oplg`, **23 entries**, CRC/SHA256/Windows-native boundary verified after restoring conditional visibility. It does not replace the published v0.1.7 package.

Native code was unchanged and was not rebuilt/retested for this UI-only change. No commit, push, publication or installation was performed.
