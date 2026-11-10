# AI Switch for OTools

## Intent and constraints

Port the local AI Switch tool from `D:/Repos/xyito/open/ai-switch` (source revision `bc1f94dd616fea8be8f96fe2355e4df7a7c3dd6c`) into the independent `otools-ai-switch` plugin. Default AI protocol listener: `127.0.0.1:10888`. Default launch mode: `panel`, without directLaunch. Desktop and remote browser management both use the current OTools SDK and host transport.

Keep account pools, credentials, model routing and protocol conversion, client configuration and snapshots, usage, image/audio/video capabilities, MCP, Skills, read-only session browsing and business settings. Exclude Vibe/PTY terminals, the original desktop shell, standalone management Web server, Tailscale and ai-switch's plugin runtime/management. SaaS operations and its public site are outside this local-tool port (implementation scope decision). Built-in image generation is kept independently of the original plugin toggle.

## Architecture and approaches

Use the existing React frontend and Rust core in an OTools cdylib plugin. The official SDK aliases Tauri invoke/listen/dialog/opener calls to the host; there is no second custom browser HTTP transport. Native dispatch returns the standard `{ok,data}` / `{ok:false,error}` contract and queues `{topic,payload}` events for `poll_events`. Native shutdown stops all plugin tasks and releases the AI port.

A Node rewrite was rejected because the mature Rust routing, protocol conversion, SQLite and atomic client-config writers already cover the required behavior. A standalone ai-switch server sidecar was rejected because it carries the redundant management server and dual runtime architecture explicitly excluded by the request.

Native state and SQLite live under `~/.otools/local/otools-ai-switch/`, never directly sharing the source application's database. Existing credential export/import is the supported migration path. Preserve upstream SQL migration history and license notices. Source copies are independent of the original checkout at build time.

The AI listener serves only model protocol endpoints, including text SSE, images, audio and videos, with local API-key authentication. It serves neither management RPC nor UI assets. Port conflicts produce a clear error instead of silently changing the default. Management RPC uses OTools native invoke, and management events use OTools native subscriptions. `plugin_http_port` reports the current AI port for host integration, without reintroducing a plugin Web server.

Persistent tasks belong to the native plugin lifecycle, not React page mounts. Do not mutate the host process proxy environment. All original desktop lifecycle actions (updates, app exit/relaunch, tray, autostart and title bar) are removed or replaced by plugin-level behavior.

## Deliverables and verification

Deliver source, retained licenses and provenance, a pinned copy of the current public SDK, build/test/pack scripts, and a Windows `.oplg` when the local toolchain permits. Other native platforms need their respective builds and must not be claimed tested here.

Verify frontend typecheck/build and retained behavior tests, SDK invocation/subscription contracts, native compilation and focused runtime/FFI tests, isolated data directory, rejected management routes and invalid model API keys, default port/conflict handling, shutdown/restart, and archive contents. Use local mock upstreams for protocol checks; do not call user accounts, change user client configs or import private source data during tests.
