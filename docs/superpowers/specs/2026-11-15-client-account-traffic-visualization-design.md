# 客户端 → 网关 → 账号池 可视化页 + 按客户端区分请求来源

日期：2026-11-15
状态：已批准（客户端后缀方案 A；无破坏性迁移；全平台总览；连线代表当前上游尝试）

## 意图与原因

用户要一个可视化页面，一眼看清「哪个 Agent 客户端正在调用哪个账号」：左侧 Agent 客户端、中间 AI Switch 网关、右侧账号池（按当前并发动态排序，最少显示 6 个，不足补齐未调用的），用虚线 + 光点流动动画表示调用关系，不同客户端不同颜色。

同时用户发现一个前提问题：现在写入所有客户端的 key 是同一把，网关无法知道请求来自哪个客户端。用户提议给 key 加后缀（`sk-ai-switch-<uuid>.zcode`），网关先拆 key 得到来源，写进请求日志，可视化页据此识别。

## 目标

1. 网关能区分「请求来自哪个客户端」，并落库到可查询的请求记录里。
2. 新增可视化页：三栏布局 + 动画连线，数据驱动。
3. 各 Agent 客户端在可视化页用官方 logo。

## 非目标

- 不改成顶部栏（本应用是左侧边栏，见下面「入口」）。
- 不为每个客户端生成独立随机 key（见方案 A/B 取舍）。
- 不动生图/视频/音频/向量模态的可视化。
- 不做历史回放时间轴（只做「当前」实时视图）。

## 反目标

- 不允许因加后缀导致现有客户端配置失效或密钥轮换/别名机制被破坏。
- 不允许可视化页在没有任何请求时空白 —— 必须仍显示账号池与客户端列。
- **非目标：持续满负荷动画或事件轮询。** SVG 光点使用 CSS/motion；活动状态通过 event bridge 即时接收，页面可见时每 3 秒读取一次后端内存 snapshot 对账（不访问数据库、不发 upstream 请求），页面隐藏时暂停。

## 现状（已核对）

- `generate_route_proxy_key()` → `sk-ai-switch-<32 hex>`（`native/src/services/route_config_service.rs:1197`）。
- key 按 **platform** 存 `route_proxy_keys(platform, proxy_key)`，轮换旧 key 进 `route_proxy_key_aliases`。
- 写入循环把**同一把** `route_proxy_key.clone()` 发给该 platform 下所有 adapter（`route_config_service.rs:169-236`），所以 ZCode / WorkBuddy / Codex CLI 不可区分。
- 网关用**精确匹配**解析 key：`get_platform_by_key`（`route_proxy_key_repository.rs:26`），带 30s 缓存（`lookup_platform_by_proxy_key`）。
- 每个 adapter 有稳定的 `client_key`（`native/src/adapters/route_config/mod.rs:174-206`，15 个）。
- 请求记录：`usage_events`，来源标签 `route_proxy`，明细在 `metadata_json`（`route_proxy_request_metadata`，`route_proxy_service.rs:2965`）。
- 实时日志：内存环形缓冲 `RouteProxyLiveLogEntry`（`route_proxy_live_log.rs:96`），**无客户端字段**。
- 并发：`route-credential-activity` 事件带 `active_request_count` / `max_concurrency`，前端已用 `setQueryData` 更新（`AccountsScreen.tsx:4150`）。
- 账号池列表：`getRoutePool(platform)`。
- 导航：左侧边栏 `AppLayout.tsx`，屏幕在 `App.tsx` 的 `implementedScreens` 注册，`NavButton` 渲染。
- Logo：只有 7 个 platform 有真实内联 SVG（`AgentIcon.tsx`）；其余 8 个客户端是字母图腾（`AgentBrandMark.tsx`）。

## 方案（key → 客户端归属）

- **A（选定）：给 platform key 加客户端后缀。** 写入时按 adapter 的 `client_key` 写 `<base>.<client_key>`；网关解析时先按 `.` 拆分，用 base 查 platform，后缀作为客户端标识记录。旋转/别名不动（base 不变）。
- **B：每客户端独立 key 行。** 新表 `(platform, client_key) → key`，撤销粒度更细，但要新迁移并分叉旋转模型，改动更大。
- **C：按 `user-agent` / 头识别。** 零配置，但可伪造且 adapter 会改写 UA，只能当辅助信号。

选 A 的理由：改动最小、复用现有解析与轮换。key base 不变，数据库 key 和 rotation alias 不变；后缀只用于客户端归因。旧配置不强制重写，未知来源明确显示为 unknown，不用可伪造的 user-agent 猜测。

## 设计

### 1. key 形态与写入

- 新增 `route_proxy_key_for_client(base, client_key) -> String`，产出 `format!("{base}.{client_key}")`；`client_key` 取自 adapter，已满足 `[a-z0-9_]`。
- 写入循环把 `route_proxy_key` 换成按 adapter 计算的带后缀值（`route_config_service.rs` 写入处的 `RouteConfigInput.route_proxy_key`）。
- **旧配置非破坏性兼容**：不主动重写用户现网文件；无后缀 key 仍有效，来源记为 `unknown`。用户下次从 AI Switch 写入该客户端配置时自然迁移到新后缀。
- 配置 adoption 必须同时匹配新后缀和旧 base/alias，避免已有无后缀配置被误判为用户手工配置而生成重复配置块。各 client adapter 已有的 alias 匹配路径要覆盖这一点。
- `route_proxy_key_aliases` 的存储和网关查找一律使用**原始 base key**，不在数据库中存带后缀 alias。解析收到的 `<base>.<client_key>` 后，只用 `<base>` 查询当前 key/rotation aliases。适配器配置写入时为当前 key 和每个历史 base alias 分别加同一个 client 后缀，adoption 同时识别 legacy raw keys。
- 已写入的老配置（无后缀）仍然有效：解析时无后缀即 `client_key = unknown`。

### 2. 网关解析

- 新增 `split_client_suffix(key) -> (&str base, Option<&str> client)`：只按**最后一个** `.` 拆，且后缀必须匹配已知 `client_key` 白名单（否则整串仍当 base，避免把含点的 key 误拆）。
- `get_platform_by_key(base)` 用 base 查；查不到再回退别名表（同样按 base）。
- `resolve_platform` 返回 `(platform, Option<client_key>)`；`forward_request` 把 client 一路带到记录处。
- 缓存 `key_cache` 的键改为 **base**，命中后后缀单独解析 —— 避免后缀把缓存打散。

### 3. 记录客户端身份

- `route_proxy_request_metadata` 增 `client_key`，每个实际上游 attempt 的 `usage_events.metadata_json` 写入 client identity；client 未识别则写 `unknown`。已有 `metadata_json` 可直接回看，无 DB migration。
- `RouteProxyLiveLogEntry` 增 `#[serde(default)] client_key: Option<String>`（老 payload 兼容），用于已结束尝试的历史记录。
- 活跃请求图不能只依赖 live log：live log 是 attempt 完成后生成，`route-credential-activity` 也只有账号 aggregate count，且 stream continuation 可以换账号却不获取新 lease。
- 新增 per-upstream-attempt lifecycle registry/event，字段至少包含 `request_id`（ingress 生成，逻辑请求稳定）、`attempt_id`（每次上游尝试唯一）、`platform`、`client_key`、`credential_id`。事件表示明确的 attempt start/end；RAII guard 在完成、失败、取消/Drop 时移除。
- registry 提供 active attempts snapshot，页面挂载时先订阅事件、再取 snapshot 并按序号/版本合并；收到重连/漏事件后重新 snapshot。由于宿主事件队列有界且可能丢旧事件，页面可见时每 3 秒从内存 snapshot 对账，并在页面重新可见时立即对账；不会查询数据库或轮询 upstream。保留既有 `active_request_count` 聚合事件作为账号并发总数。
- attempt 生命周期覆盖真正的上游请求（send 到 response body 结束/失败），不把本地模型列表、CORS、计数 token、模型测试、恢复探针或本地 `route_once` 操作画成外部 Agent 流量。普通重试/账号 failover 结束旧 attempt 后启动新 attempt；流内 continuation 换账号也必须替换活跃 attempt guard 与账号关联。
- `ProxyRequestRow` 不另加字段；其 `metadata_json` 已作为来源，使用统计详情面板读取并展示可选的 `client_key`。

### 4. 可视化页（新屏幕 `Traffic`）

- 三栏：左「客户端」、中「AI Switch 网关」、右「账号池」。
- **范围：所有平台总览**（Codex、Claude、Gemini、Grok、OpenCode、OpenClaw、Hermes 等当前支持平台同屏显示），不要求先选平台。客户端来自所有平台的 `listConfigWriteClients(platform)` 合并、按 client_key 去重；账号来自所有平台账号池合并、按 credential ID 去重。每项客户端 = 官方 logo（如许可不明用现图腾）+ 名称 + 当前活跃请求数。
- **网关列**：单一节点，显示服务状态（`routeProxyQuery`）+ 当前被图可视化的 active upstream attempt 总数。
- **账号池列**：所有平台当前激活 pool 的账号 ∪ 当前 active attempts 里的 credential ID（被调用的非 pool 账号作为活动项保留）；按 active attempt 数降序，至少 6 个，不足用当前 pool 空闲账号补齐；未调用账号先按 `route_priority`、再按 `display_name`、ID 稳定排序；被调用的账号高亮并显示并发数。账号静态目录通过只读 `list_route_proxy_account_summaries(platform)` 获取，仅返回 id、platform、display_name、route_priority、kind、status、max_concurrency、active_request_count；不返回 token、secret、config、模型或用量字段。
- **连线**：始终保留客户端→网关、网关→账号的淡色静态虚线骨架；没有真实请求时无光点、无动画。真实请求的对应线路高亮为客户端颜色并出现沿路径运动的光点；结束后恢复静态虚线。活动线路表示当前 upstream attempt，retry/failover/continuation 换号时随之迁移；静态线路只是可路由拓扑，不代表正在调用。
- 客户端按 client_key 去重后显示当前活跃**逻辑请求数**（按 request_id 去重；同一逻辑请求因 retry/continuation 同时交接时不重复计数）；账号和连接按 active attempt 数计数。
- **数据**：启动时枚举全部可见平台（来自 `listPlatformCapabilities()` 中支持通用 API routing 的平台），分别获取可写客户端、当前激活账号池成员和路由服务状态；订阅 lifecycle events 并读取 active snapshot。事件即时更新；页面可见时每 3 秒用内存快照对账，并在重新可见时立即对账（应对宿主有界事件队列丢失结束事件）。不查数据库、不轮询 upstream。无请求时静态显示客户端、网关和当前池成员。
- **性能**：光点用 CSS/motion 动画，不做逐帧 React 重渲染；事件订阅在页面卸载时 unsubscribe。客户端和账号基础列表可使用 React Query 加载/缓存。

### 5. 入口

- `AppLayout.tsx` 系统分区增加「实时路由」NavButton；通用账号页顶部工具栏增加同名入口，即使服务未启动也可打开静态拓扑。`App.tsx` 注册和渲染 `Traffic`；i18n 使用 `nav.traffic`（en/zh）。
- 保持原默认首页，不自动切换到可视化页。

### 6. 官方 logo

- 新建 `src/components/brand/AgentLogo.tsx`，内联/本地 SVG 或 PNG，离线可用，按 `client_key` 分派。
- 用户明确要求官方 logo；每个 client 优先取厂商官网/CDN 或官方项目仓库中的资源，随包本地化。研究核验表见计划文档；具体覆盖与许可条款逐项标注。
- 项目代码许可证不自动等于商标/logo 再分发授权。对服务条款明确要求先获书面授权或授权不清的 vendor marks，不随包嵌入；保留现有图腾/平台官方图标并在代码注释或 asset README 标明未用官方版本的原因。
- 不引入运行时网络请求；logo 文件随包，离线可用。

## 留给实现者的

- 已做决定：全平台同屏总览；账号按 ID 去重；当前上游 attempt 决定活动连线；客户端活跃数按 request_id 去重；空闲账号按 `route_priority`、`display_name`、ID 稳定排序；旧配置不自动重写；活动页仅使用脱敏账号摘要 API。
