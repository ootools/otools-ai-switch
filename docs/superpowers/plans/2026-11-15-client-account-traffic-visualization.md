# 客户端流量可视化实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use `executing-plans` or `subagent-driven-development` to implement task-by-task. Steps use checkbox syntax for tracking.

**Goal:** 网关按写入 key 后缀识别 Agent 客户端；新增全平台实时页面，显示每个客户端当前 upstream attempt 正在使用哪个账号。

**Architecture:** 每 platform 原有 base key 和 rotation aliases 不变，写入配置时附加 `.client_key`；请求入口仅拆解已注册 client 后缀。后端常驻维护有序号、可快照的活动 upstream attempt registry；request metadata/live log 记录来源。React 页订阅快照和增量事件，合并所有支持平台的客户端与当前 pool 成员目录并绘制动画连接。事件即时更新；页面可见时每 3 秒从内存快照对账，修正宿主有界队列可能丢掉的末尾事件。

**Tech Stack:** Rust/Tokio/Serde/SQLx、React/TypeScript、TanStack Query、OTools transport、CSS/motion。

**Spec:** `docs/superpowers/specs/2026-11-15-client-account-traffic-visualization-design.md`

## Global Constraints

- 新 key 格式为 `<existing-base-key>.<client_key>`；数据库只存原有 base key/base aliases。
- 旧无后缀配置继续工作，不强制改写现网文件；来源标为 `unknown`。
- adapter adoption 同时匹配 suffixed current/aliases 和 legacy raw current/aliases，不能因迁移创建重复配置。
- 只把外部通用路由的 upstream attempts 纳入活动图；CORS、模型列表、count_tokens、内部模型测试/健康探测、`route_once` 和内部 scoped requests 不冒充外部 Agent 流量。
- 活动连线代表一个当前 upstream attempt；retry/failover/stream continuation 切账号时旧 attempt 结束、新 attempt 建立。
- 后端常驻维护快照/事件；不能靠 completed live-log 推断活动请求。事件负责即时更新；由于宿主事件队列有界可能丢掉最后一个 finish，页面可见时每 3 秒用内存快照对账，并在重新可见时立即对账；不访问数据库、不轮询 upstream。
- 所有支持 `generic_api_routing` 的平台同屏；客户端按 client_key 去重；账号按 credential ID 去重，只显示当前 pool members 和正在承接请求的非成员账号。空闲账号按 `route_priority`、`display_name`、ID 稳定排序；至少显示 6 个可用账号，不足 6 个时显示全部。
- Logo 离线可用。条款要求授权或再分发许可不清的 vendor mark 不随包，保留已有官方平台 icon/字母图腾，并记录来源与理由。
- 测试先红后绿；验证只覆盖本次相关范围，保留不相关工作区更改。

## Review Focus

- 同 platform 多 client 共用 base、suffix 不同；不同 suffix 必须解析同一 platform、不同 client。
- 旧 raw key 与 rotation aliases 继续解析；旧配置能被原位 adoption。
- 快照与事件交错、事件序号断档、延迟订阅卸载不能漏/复活活动 attempt；发生 gap 时重新取快照。
- stream cancel/drop 和 continuation 切账号必须清理/迁移活动行；同一逻辑请求 request_id 稳定、attempt_id 唯一。
- 空/未知/格式错误 suffix 不能被静默剥离绕过 key 校验；`client_key` 必须来自注册清单。

---

## Task 1: 配置写入按 client 加后缀并保留 adoption

**Files:** `native/src/adapters/route_config/mod.rs`; `native/src/services/route_config_service.rs`; `native/src/adapters/route_config/{zcode,workbuddy,qoder_cli}.rs`; colocated Rust tests.

**Produces:** `route_proxy_key_for_client(base_key: &str, client_key: &str) -> String`, 输出 `"{base_key}.{client_key}"`。数据库 key/aliases 仍为 raw base。

- [x] **Step 1: 写 RED tests。** helper 输出后缀；同 platform 的 codex 与 zcode 输出不同 suffix；三个 key-aware adapters 分别识别 suffixed current、legacy raw current、suffixed historical base alias、raw historical alias；写入 legacy entry 后 provider/rule 数不增加。
- [x] **Step 2: 验证失败。** `cd native && cargo test route_proxy_key_for_client`，再运行三个 adapter 的新 adoption 用例；预期因 helper/匹配分支缺失而失败。
- [x] **Step 3: 实现 helper 和 adapter-specific write input。** `route_config_service.rs` 循环内用 `adapter.client_key()` 生成当前 key；`RouteConfigInput.route_proxy_key_aliases` 继续携带 raw base aliases，不生成 suffixed alias 存储或传输。
- [x] **Step 4: 更新 adoption。** ZCode/WorkBuddy/Qoder 各自根据 `self.client_key()` 比较 suffixed current key、raw current base、每个 suffixed alias 和 raw alias；保留 target URL/provider 条件。
- [x] **Step 5: 验证。** `cd native && cargo test route_proxy_key_for_client && cargo test adoption_recognizes_suffixed_key_and_legacy_alias && cargo test route_config_service`；预期通过且 base key 跨写入稳定。

## Task 2: 请求 key 后缀解析并记录客户端来源

**Files:** `native/src/services/route_proxy_service.rs`; `native/src/services/route_proxy_live_log.rs`; `src/lib/usageMetadata.ts`; `src/components/accounts/UsageOverviewPanel.tsx`; associated tests.

**Produces:** `split_route_proxy_client_key(key: &str) -> (&str, Option<&'static str>)`; live log optional defaulted `client_key`; `ParsedUsageMetadata.clientKey: string | null`.

- [x] **Step 1: 写 RED tests。** 覆盖所有注册 client keys、无 suffix、空 suffix、未知 suffix、不合法 base 和多个点；未知 suffix 必须保留整 key，不能 strip 后通过认证。
- [x] **Step 2: 验证失败。** `cd native && cargo test split_route_proxy_client_key`；预期失败。
- [x] **Step 3: 实现严格拆分与 base lookup。** 只接受 adapter registry 中的 client key；平台解析、cache、当前 key/aliases 查询都只用 base。旧 key 保持有效且 client identity 显示为 `unknown`（metadata 字符串用 `unknown`，typed live field 使用 `None`）。
- [x] **Step 4: 将 client_key 贯穿 `forward_request`。** 所有实际 upstream attempt 的 `route_proxy_request_metadata` 和 `emit_live_log` 都记录来源；未识别写 `unknown`。给 `RouteProxyLiveLogEntry` 加 `#[serde(default)] client_key: Option<String>`，旧 JSON 可读。
- [x] **Step 5: 更新使用详情。** `parseUsageMetadata` 解析 client_key；usage detail 显示 Client（无字段时 `unknown`），raw formatted metadata 仍可见。
- [x] **Step 6: 验证。** `cd native && cargo test route_proxy_service`; `pnpm vitest run tests/UsageOverviewPanel.test.tsx`（并运行相关 usageMetadata 用例）；预期旧 live-log 和缺字段 metadata 兼容。

## Task 3: 活动 upstream attempt registry、事件和快照命令

**Files:** Create `native/src/services/route_proxy_active_attempts.rs`; modify `native/src/services/mod.rs`, `native/src/services/route_proxy_service.rs`, `native/src/runtime.rs`, `native/src/web/handlers/mod.rs`, plugin RPC dispatch; add types/wrapper in `src/lib/api/{types.ts,client.ts}`.

**Produces:**
- `ActiveProxyAttempt { request_id, attempt_id, platform, client_key, credential_id, credential_name, started_at }`。
- `ActiveProxyAttemptEvent { sequence, action: "started" | "finished", attempt }`，topic `route-proxy-active-attempt`。
- `ActiveProxyAttemptSnapshot { sequence, active }`，command `get_route_proxy_active_attempts`。
- `RouteProxyActiveAttemptRegistry::begin(request_id, platform, client_key, credential_id, credential_name, started_at) -> ActiveProxyAttemptGuard` creates a UUID `attempt_id`; `snapshot() -> ActiveProxyAttemptSnapshot` returns current sequence and active values.

- [x] **Step 1: 写 registry RED tests。** begin 插入+发 started，guard drop 移除+发 finished；sequence 单调严格递增；snapshot 的 sequence 和 active 集合在同一锁边界取得；emitter 能收到消息。registry 自己生成唯一 attempt_id；调用者不得注入重复 id。
- [x] **Step 2: 验证失败。** `cd native && cargo test route_proxy_active_attempts`；预期 module 不存在。
- [x] **Step 3: 实现有界、同步 registry。** mutex 保护 map/sequence/emitter；start/end 原子更新并 emit，不跨 await 持锁；RAII guard 保证取消/drop 清理。活动 map 持续存在，不按页面是否打开开关。
- [x] **Step 4: 接到 runtime。** `RouteProxyRuntimeState` 拥有 registry；runtime 启动时设置现有 event bridge emitter；proxy state clone registry；提供快照方法。
- [x] **Step 5: 暴露命令和 TS 类型。** native handler + OTools RPC dispatch 注册 snapshot command；frontend `getRouteProxyActiveAttempts()` 调用该命令。事件经 transport event bridge 订阅，不增加 viewer refcount。
- [x] **Step 6: 验证。** registry/command tests 通过，序列化字段符合 TS 类型。

## Task 4: 贯穿实际请求、failover 和 stream continuation

**Files:** `native/src/services/route_proxy_service.rs`; focused Rust tests。

- [x] **Step 1: 写 RED lifecycle tests。** 每个外部 buffered upstream send 与 primary stream 各 start/end 一条 attempt；同一 logical request retries 保持 request_id、attempt_id 不复用；retry backoff 期间没有活动 edge。
- [x] **Step 2: 增加取消和切账号 tests。** drop/cancel stream 清除活动 row；stream continuation 换 credential 时 old attempt finished、新 attempt started 且新 edge 指向新 credential；普通 failover 同样成立。
- [x] **Step 3: 验证失败。** 运行新增 proxy lifecycle 过滤用例；预期缺少事件/清理行为。
- [x] **Step 4: Instrument ingress/buffered send。** ingress 为外部 request 生成一次 request_id；每次将要发出的真实 upstream HTTP 请求生成唯一 attempt_id；attempt guard 在响应体处理完成/失败后 drop，retry sleep 前 drop。
- [x] **Step 5: Instrument stream owner/continuation。** primary attempt guard 随 stream completion/pump 生命周期保存并在正常 finish/drop 时释放；`StreamPump::try_continue` 切换 credential 前结束旧 guard，continuation send 前创建新 guard，保留 request_id/client_key。
- [x] **Step 6: 排除内部路径。** CORS、模型列表、count_tokens、模型测试/恢复探针、route_once 和 access-scoped 内部流量不建立外部 client 边；refresh/build 失败且未发 HTTP 不建立 attempt。
- [x] **Step 7: 验证。** `cd native && cargo test route_proxy_service && cargo test route_proxy_active_attempts`；无事件残留且路由行为不变。

## Task 5: Logo 组件和可用资源记录

**Files:** Create `src/components/brand/AgentLogo.tsx`; create `src/components/brand/agent-logo-sources.md`; local assets only when approved; `tests/AgentLogo.test.tsx`.

- [x] **Step 1: 写 RED render tests。** 所有注册 client key 有可访问名称；支持许可的 local asset 使用 local file；许可不清/有明文限制的品牌使用现有 monogram/platform icon；未知 client 有 neutral fallback。
- [x] **Step 2: 验证失败。** `pnpm vitest run tests/AgentLogo.test.tsx`；预期 module 不存在。
- [x] **Step 3: 核验并记录资源。** 使用官方站点/CDN 或官方 repo 的实际文件并记录来源、许可/条款、检索日期。代码 license 不视为商标授权；Tencent/WorkBuddy/CodeBuddy 等条款要求事先授权的资源不打包。
- [x] **Step 4: 实现离线映射。** 先复用 `AgentIcon` 的官方 platform SVG；许可可接受时随包加官方 client logo；其他用 `AgentBrandMark`/官方平台图标并注明理由。运行时不请求外网。
- [x] **Step 5: 验证。** 所有 15 个注册 client 与 unknown fallback 测试通过，检查无 runtime fetch。

## Task 6: 全平台 Traffic UI、动态边和导航

**Files:** Create `src/screens/TrafficScreen.tsx`, `src/screens/TrafficScreen.css`; modify `src/lib/api/{types.ts,client.ts}`, `src/App.tsx`, `src/components/layout/AppLayout.tsx`, `src/lib/i18n.tsx`; add `tests/TrafficScreen.test.tsx` and nav tests.

**Interfaces:** `listPlatformCapabilities()`, `listConfigWriteClients(platform)`, `listRouteProxyAccountSummaries("codex")`, `getRouteProxyPoolMemberIds()`, `getRouteProxyStatus()`, `getRouteProxyActiveAttempts()`, `getTransport().subscribe("route-proxy-active-attempt", handler)`。统一池只查询一次账号摘要及当前成员 ID；不读取完整凭据或 `getRoutePool` 的请求历史。

`RouteProxyAccountSummary` returns only `id`, `platform`, `display_name`, `route_priority`, `kind`, `status`, `max_concurrency`, `active_request_count`; no secrets/config/model/usage fields.

- [x] **Step 0: 写摘要 API 的 RED tests。** 增加 `RouteProxyAccountSummary` 与只读 repository/service query；测试 JSON 只含 8 个计划字段且不含 `secret_payload_json`、`config_json`、`preview_json`、models 或 usage；测试 active_request_count 来自内存 activity registry。
- [x] **Step 1: 写 RED screen/state tests。** 多平台 client/account、相同 client_key/credential ID 去重，active snapshot、start/end、重复/乱序/gap event、无活动请求仍显示清单。断言同一 client 多 adapter attempt 用 request_id 去重、账号和连线显示 attempt 数，静态池成员从 `getRoutePool().account_ids` 选出，最少 6 个账号。
- [x] **Step 2: 写 subscription race tests。** 订阅 Promise 延迟至 unmount 后 resolve 时立即 unsubscribe；snapshot 加载期间缓存事件，加载 snapshot N 后只应用 `sequence > N`；检测 sequence gap 后重新取 snapshot。
- [x] **Step 3: 验证失败。** `pnpm vitest run tests/TrafficScreen.test.tsx`；预期缺 screen/API wrapper。
- [x] **Step 4: Add the summary API and catalog loading.** Native read-only query returns only the eight declared fields and reads current active counts from activity registry. Frontend selects platforms with generic routing available; per platform parallel-fetch config clients, summary rows and `getRoutePool(platform)`. Static account catalog is pool `account_ids` joined against summaries; merge IDs. Add active nonmembers from snapshot; when summary lookup has no row, show unknown account placeholder. Never call/cache `listRouteCredentials` from Traffic.
- [x] **Step 5: Implement subscription and sequence merge.** 先 attach event listener 并缓冲事件；再请求 active snapshot，安装 snapshot 后只 replay 更高 sequence；snapshot response 若较旧不得覆盖 listener 已应用的更高序号事件。后续忽略重复/旧 sequence，发现 sequence gap 时立即重新取 snapshot。页面可见时每 3 秒调用内存快照对账，`visibilitychange` 切回可见时立即对账；不访问数据库/不轮询 upstream。disposed 后忽略事件并释放 unsubscribe。
- [x] **Step 6: 实现三栏图。** clients → gateway → credentials。客户端数为不同 active request_id 数；账号列按 active attempt 数降序，空闲补位排序为 route_priority、display_name、ID；至少 6 个或显示全部可用账号。多 attempts 同一 client/account pair 合成一条边与 attempt 数 badge。SVG 虚线、按 client 品牌色的 CSS/motion 光点；`prefers-reduced-motion` 下静止显示边与数值。无请求时仍显示完整客户端、网关、账号目录。
- [x] **Step 7: 注册入口和双语标签。** `App.tsx` 注册/渲染 `Traffic`；`AppLayout.tsx` 系统栏加侧边 NavButton；i18n 增 `nav.traffic` en/zh；不改变默认首页。
- [x] **Step 8: 验证 UI。** `pnpm vitest run tests/TrafficScreen.test.tsx tests/AgentLogo.test.tsx <nav tests> && pnpm typecheck`；断言 all-platform view 与 cleanup 通过。

## Task 7: 全量验证

- [x] **Step 1:** `pnpm typecheck && pnpm test:run`；预期全量通过。
- [x] **Step 2:** `cd native && cargo test --lib`；预期通过（既有 ignored tests 不变）。
- [x] **Step 3:** `pnpm build`；预期 Traffic 与允许的本地 logo assets 正常打包。
- [x] **Step 4:** `node scripts/verify-plugin.mjs && node --test scripts/package.test.mjs`；预期 plugin boundary/包内容校验通过。
- [x] **Step 5:** `git diff --check` 通过；独立最终审查已完成，发现的快照重启、停机代次、活动账号摘要和目录刷新问题均经回归修复。最终前端 910 项、native 2009 项通过，构建和包校验通过；未推送、发布或改写已安装配置。
