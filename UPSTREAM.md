# Source provenance

This independent OTools plugin adapts AI Switch's local AI management features.

| Component | Source | Snapshot |
| --- | --- | --- |
| AI Switch | https://github.com/ai-switch/ai-switch | `bc1f94dd616fea8be8f96fe2355e4df7a7c3dd6c` (0.17.1) |
| OTools plugin SDK | https://github.com/ootools/otools-plugin-sdk | `e82c50ae5c1dbb73d1b5eeea1faafd13e6e03de3` (2026-11-10) |

Source files were copied from the user's local checkouts. Frontend build configuration is adapted for the plugin; the source application's local development Vite proxy configuration is not used. The SDK snapshot is checked in under `vendor/otools-plugin-sdk`, so building the plugin does not depend on neighboring repositories.

Local SDK compatibility patches are limited to remote file dialogs and panel lifecycle cleanup. Remote file selection uses the SDK host file tree rather than a native dialog on the server; panel close releases page subscriptions while preserving the AI service. These patches are kept inside the SDK boundary so business components retain the same APIs in both host runtimes.

Changes replace the application shell and desktop/web transport with the OTools SDK and native plugin ABI. The plugin excludes Vibe terminals, Tailscale, standalone management Web service, the original plugin runtime and SaaS site/operations. AI model protocol serving remains in the native backend with default port 10888. Storage is isolated from the original app.

AI Switch and the SDK are MIT licensed; see `LICENSE` and `LICENSES/otools-plugin-sdk-MIT.txt`. The upstream MCP/Skills portions retain Apache-2.0 notices; see `THIRD_PARTY_NOTICES.md` and `LICENSES/Apache-2.0.txt`.
