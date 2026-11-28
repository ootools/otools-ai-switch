# Compact account processing settings

Approved: 2026-11-27. The user chose the compact settings list over a retained flowchart.

## Intent
Replace the account request-processing canvas, whose wrapped/stretching cards obscure order and waste space, with a quiet, compact settings editor inspired by Magpie's settings organization.

## Approaches
- Selected: request/response tabs and a vertical ordered list; expand one row to edit its parameters. Fits the existing narrow account dialog without horizontal scrolling.
- Rejected: compact flow nodes and a separate parameter panel. Better for a visual path but less suitable for this dialog's width.

## Scope and behavior
- Show only one processing direction at a time, with keyboard-accessible tabs.
- Each row shows its original sequence number, name, enabled state, and a single-line parameter summary.
- All rows start collapsed; expand only one at a time. Put reorder/remove actions in the expanded details.
- Keep each direction's passthrough/process selector. Passthrough hides the editor without clearing the chain or its settings.
- Preserve the PipelineFlowEditor props and pipeline persistence shape, plugin order, disabled instances, unavailable plugins, and unknown config keys.
- Give enabled state a real switch. Adding a disabled existing plugin re-enables that instance and keeps its config, rather than creating a duplicate.
- Keep all existing config field kinds (select, number, textarea, bool), native controls, and save flow.
- Support narrow layouts, light/dark themes, and keyboard focus. Do not change native runtime behavior or add dependencies.

## Visual direction
One bordered section; underlined direction tabs; flat rows separated by rules; restrained neutral colors; compact status switches. No animated connectors, per-plugin cards, or permanently exposed parameter forms. Reuse existing typography/theme utilities.

## Verification
Dedicated behavior tests, updated account-dialog integration test, full frontend suite, TypeScript check/build, and rendered browser checks at narrow/normal widths and in dark mode.