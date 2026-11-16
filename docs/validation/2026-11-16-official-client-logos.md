# Official client-logo correction — 2026-11-16

## Root cause

WorkBuddy and five other third-party-client entries still used W/CB/Z/Q monogram fallback artwork. They had no official-asset mapping in AgentLogo; this was not a browser cache, installed-version or failed-image request problem. The original generic test asserted only an SVG/image existed and a local accessible name, so it did not distinguish an official logo from a letter placeholder.

## Correction

- WorkBuddy and WorkBuddy AI use their byte-identical official green Buddy favicon (`workbuddy.svg`, 40×40).
- CodeBuddy CLI uses the original purple Buddy favicon explicitly linked by its official CLI page (`codebuddy.svg`, 40×40), not a recolored WorkBuddy icon.
- ZCode uses the official angular Z application icon from pinned repository revision `fec975905a2abd4c07e35c40d51fa097a2258ec9` (`zcode.png`, 128×128).
- Qoder CLI and Qoder CN CLI share their byte-identical official square `favIcon.svg` (`qoder.svg`, 206×206).
- Assets remain unmodified, offline and shared by client setup and live-routing views. Source URLs and SHA256 hashes are recorded in `src/components/brand/agent-logo-sources.md`.
- Existing native-client artwork was checked; Gemini CLI and OpenClaw match their first-party small icons. No additional native-platform replacement was made. Exact full-asset provenance for every existing native vector is not claimed.
- Original ZCode LICENSE and NOTICE are preserved separately in LICENSES. Tencent/Qoder official provenance does not establish blanket trademark/redistribution consent; relevant terms and this limitation are documented without asserting permission.

## Validation

- WorkBuddy and subsequent ZCode/CodeBuddy/Qoder mapping regressions failed before replacement and passed afterward.
- AgentLogo tests: **29 passed**, including exact official file/hash assertions for all six corrected client keys through ClientIcon.
- Full frontend suite: **78 files / 931 tests passed**.
- Typecheck and production build passed; package regressions **5/5 passed**. Existing OCRAD/eval/chunk-size warnings remain.
- Browser: all six corrected entries load their expected local artwork with positive natural dimensions and no SVG text monogram; WorkBuddy/AI show the green official symbol. Client setup separately verified WorkBuddy image loading. Display-only preview data was used, not installed account data.
- Local validation package: `artifacts/brand-fix-local-package/otools-ai-switch-0.1.6.oplg`, **23 entries**, CRC/SHA256/native boundary verified. All four new artwork files match upstream hashes inside `dist/brand/`; ZCode notices are present.

## Scope

This is an uncommitted local correction after the public v0.1.6 release. The existing published v0.1.6 assets remain unchanged. No new public release, remote push, installation or native-code change was performed.
