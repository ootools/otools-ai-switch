# Codex++ / Magpie 账号导入 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 「导入其他客户端」新增 codex-plus 与 magpie 两个来源，API 供应商与 OAuth 订阅账号都能预览、勾选、去重导入。

**Architecture:** 共享的 preview/import 服务按 client 分发到 reader 模块；三个 reader 统一产出 `PreparedExternalEntry`（Api / Official / Broken / Skipped 四类），官方条目复用 `parse_cpa_text` 解析并按 `kind: "official"` 落库，覆盖规则改为同 kind 才允许。

**Tech Stack:** Rust (sqlx/serde_json/tokio, native/) + TypeScript React (vitest, src/)。

**Spec:** `docs/superpowers/specs/2026-11-19-external-import-codexpp-magpie-design.md`

## Global Constraints

- 新 client 常量：`EXTERNAL_CLIENT_CODEX_PLUS = "codex-plus"`、`EXTERNAL_CLIENT_MAGPIE = "magpie"`（一经发布不可改值）。
- 预览内一切密钥经 `deeplink_service::mask_api_key` 脱敏；序列化后的预览不得含明文。
- 条目上限沿用 `EXTERNAL_CLIENT_MAX_ITEMS`（2000），按 reader 产出总数计。
- 导入过程绝不向任何厂商 token 端点发请求；OAuth 只落快照。
- 来源 id 恒带 kind/协议后缀：`relay:<id>:api`、`relay:<id>:official`、`provider:<id>:<protocol>`、`claude:<id>`、`login:<id 或 agent:user>`；Codex++ 直连与网关共用 `relay:<id>` 命名空间。
- 覆盖只允许同 kind：api ↔ official 互撞报 `external_import.conflicting_local_account`。
- 统一账号池：存储平台恒 codex，逐条 platform 只推导 provider 与列表归属。
- 命令：前端 `pnpm typecheck` / `pnpm test:run`；native `cargo test --manifest-path native/Cargo.toml <filter>`。

## Review Focus

- **OAuth 快照导入绝不触发刷新**：导入 Codex/Magpie 官方条目时 secret payload 必须等于快照原文 token；测试钉死（Task 6 的断言 payload 键值），任何实现引入 HTTP 刷新都会失败。
- **预览脱敏**：official 条目的 access_token 也要打码（Task 6 序列化断言 `!contains("sk-ant-")` 模式）。
- **来源 id 稳定性**：用户在来源工具增删协议 URL、relay 搬家后重导必须覆盖而非新增（Task 4/5/6 的 round-trip 测试）。
- **cc-switch 零回归**：Task 3 纯重构后全部既有 `external_client_import_service` / `cc_switch_import_service` 测试必须原样通过。
- **上限与病态文件**：超过 2000 条、非法 JSON、半截文件 → 明确报错不是 panic（Task 4/5）。

---

### Task 1: 预览模型扩展（client 常量、kind、unsupported 计数）

**Files:**
- Modify: `native/src/models/external_client_import.rs`

**Interfaces:**
- Produces: `EXTERNAL_CLIENT_CODEX_PLUS: &str`、`EXTERNAL_CLIENT_MAGPIE: &str`；`ExternalClientAccountPreviewItem.kind: String`（`"api" | "official"`）；`ExternalClientImportPreviewCounts.official: usize`、`.unsupported: usize`、`.unsupported_counts: BTreeMap<String, usize>`（均 serde default，旧 JSON 可反序列化）。

- [ ] **Step 1: 写失败测试**（同文件 `#[cfg(test)]`）：反序列化缺少新字段的旧 JSON → `kind == "api"`、三个新计数为 0；序列化含新字段。常量断言 `EXTERNAL_CLIENT_CODEX_PLUS == "codex-plus"`、`EXTERNAL_CLIENT_MAGPIE == "magpie"`。
- [ ] **Step 2:** `cargo test --manifest-path native/Cargo.toml external_client_import` → FAIL（字段不存在）。
- [ ] **Step 3:** 实现常量与字段：`kind` 带 `#[serde(default = "default_api_kind")]`，三个计数 `#[serde(default)]`。
- [ ] **Step 4:** 同上命令 → PASS。
- [ ] **Step 5:** `git add native/src/models/external_client_import.rs && git commit -m "外部导入预览模型增加 kind 与 unsupported 计数"`

### Task 2: ExtractedApiCredential 增加 extra_headers

**Files:**
- Modify: `native/src/services/cc_switch_import_service.rs`（struct 定义）
- Modify: `native/src/services/external_client_import_service.rs`（`build_payload`）

**Interfaces:**
- Produces: `ExtractedApiCredential.extra_headers: Vec<(String, String)>`（按序自定义头）。cc-switch 提取路径恒返回空 vec，现有行为不变。

- [ ] **Step 1: 写失败测试**（`external_client_import_service` 测试内）：构造 `ExtractedApiCredential`（user_agent `Some("ua/1")`、`extra_headers = vec![("X-Debug","1"),("X-Trace","2")]`），断言 `build_payload` 产出的 `config["headers"]` 同时含 `"User-Agent":"ua/1"`、`"X-Debug":"1"`、`"X-Trace":"2"`，且键序保持自定义头在前。
- [ ] **Step 2:** `cargo test --manifest-path native/Cargo.toml external_client_import` → FAIL。
- [ ] **Step 3:** struct 加字段（cc-switch 两处构造点补 `extra_headers: Vec::new()`）；`build_payload` 改为：先放 `extra_headers`，再在无同名键时写 `"User-Agent"`（headers map 用 `serde_json::Map`）。
- [ ] **Step 4:** 同上命令 → PASS；`cargo test --manifest-path native/Cargo.toml cc_switch` → PASS（零回归）。
- [ ] **Step 5:** `git commit -m "外部导入 API 条目支持自定义请求头"`（含两个文件）。

### Task 3: 统一 PreparedExternalEntry，cc-switch 适配（纯重构）

**Files:**
- Modify: `native/src/services/external_client_import_service.rs`
- Modify: `native/src/services/cc_switch_import_service.rs`（新增一个适配函数）

**Interfaces:**
- Produces（后续 reader 都按此产出）:
  ```rust
  pub struct PreparedExternalEntry { pub source_id: String, pub body: PreparedBody }
  pub enum PreparedBody {
      Api(ExtractedApiCredential),
      Official(ParsedOfficialCredential),   // cpa_import_service::ParsedOfficialCredential
      Broken { display_name: String, issue_code: &'static str, platform_hint: Option<PlatformId> },
      Skipped(SkipBucket),
  }
  pub enum SkipBucket { OtherPlatform(Option<PlatformId>, String), Unsupported(String) }
  ```
- Produces: `cc_switch::prepare_entries(path: &Path) -> Result<Vec<PreparedExternalEntry>, AppError>`（= read_providers + 逐条 extract 包装；`OfficialLogin`/`ApiKeyMissing`/`BaseUrlMissing`/`BaseUrlInvalid` → `Broken`，`PlatformUnsupported` → `Skipped(OtherPlatform(None, app_type))`）。
- Produces: `codex_plus::resolve_source_dir(source_path: Option<&str>) -> Result<CodexPlusSourceDir, AppError>`、`magpie::resolve_source_dir(...) -> Result<PathBuf, AppError>` 的签名在本任务先以空实现占位（Task 4/5 填充），service 的 `resolve_entries(client, source_path) -> Result<(Vec<PreparedExternalEntry>, String), AppError>` 分发成形。

- [ ] **Step 1: 确认现状测试全绿**：`cargo test --manifest-path native/Cargo.toml external_client cc_switch` → PASS（基线）。
- [ ] **Step 2: 重构**：`classify` 改为消费 `&PreparedExternalEntry`；`Api` 分支沿用现有逻辑（platform 过滤、overwrite_target、disposition）；`Official` 分支本任务先按 `Broken` 占位实现最小逻辑（真实落库在 Task 6）；`Broken` 沿用原 `Err(issue)` 三段判断；`Skipped` → `Classification::OtherPlatform`（OtherPlatform 桶标签取第二元素；Unsupported 桶本任务先并入 `other_platform_counts` 临时键 `"unsupported:<label>"`，Task 6 换真计数）。preview/import 两处循环改为 `resolve_entries`。
- [ ] **Step 3:** `cargo test --manifest-path native/Cargo.toml external_client cc_switch` → PASS（测试代码不改一行；如有失败，修实现不改断言）。
- [ ] **Step 4:** `git commit -m "外部导入服务重构为统一条目接口"`

### Task 4: codex-plus reader

**Files:**
- Create: `native/src/services/codex_plus_import_service.rs`（`native/src/services/mod.rs` 注册）

**Interfaces:**
- Consumes: `cc_switch::prepare_entries` 不复用；复用 `cc_switch::extract_api_credential`（对 `claude-providers.json` 条目合成 `ExternalClientProvider{app_type:"claude", settings_config, ..}` 调用）、`cpa_import_service::parse_cpa_text`、`crate::models::platform::{PlatformId, ApiDialect}`、`EXTERNAL_CLIENT_MAX_ITEMS`。
- Produces: `pub fn resolve_source_dir(source_path: Option<&str>) -> Result<PathBuf, AppError>`（显式目录必须存在；缺省 `~/.codex-session-delete`，`CODEX_PLUS_STATE_DIR` 覆盖，`~/settings.path` 指针仅缺省时生效）与 `pub async fn prepare_entries(dir: &Path) -> Result<Vec<PreparedExternalEntry>, AppError>`。
- 映射规则（spec 为准，此处锁定值）：`protocol` `responses`→`(PlatformId::Codex, ApiDialect::OpenAiResponses)`、`chatCompletions`→`(Codex, OpenAi)`、`anthropicMessages`→`(Claude, Anthropic)`；`base_url` = `upstreamBaseUrl` 优先，否则 `baseUrl`，两者皆空或解析为回环地址 → `Broken{issue_code:"external_import.upstream_missing"}`；api key 链 `apiKey` → `apiKeys[activeApiKeyId]` → 首项，`noAuth=true` → `Broken{"external_import.api_key_missing"}`；`relayMode` `aggregate` → `Skipped(Unsupported("aggregate"))`、`official` → authContents 解析（见下）；`officialMixApiKey` 同时有 key 与 authContents → 产出 `relay:<id>:api` 与 `relay:<id>:official` 两条。
- official 归一：`authContents` 文本 parse 成 JSON；为 `{auth_mode, tokens:{...}}` 形状时平铺 tokens 至顶层并加 `"type":"codex"`，`officialAccountRef` 非空且自身无 authContents 时先在 gateway.json 同 id relay 找 authContents；parse 失败/无 token → `Broken{"external_import.official_snapshot_missing"}`（悬空引用同此 code）；`parse_cpa_text` 失败 → `Broken{"external_import.official_snapshot_invalid"}`。
- claude 条目：`claude-providers.json` 的 `providers[]`（`{id,name,settingsConfig}`）→ `claude:<id>`，extract 失败按 cc-switch 同款 `Broken`/`Skipped` 规则。

- [ ] **Step 1: 写失败测试**（同文件，fixture 用 `tempfile` 写盘，形状取自上游源码）：① camelCase `RelayProfile` 全字段提取断言（key 链、upstream 优先、headers 合并、modelList 换行拆分为 from==to 映射）；② `relayMode:"official"` + `authContents` → `Official` 条目且 `parse_cpa_text` 产物 platform=codex；③ official + officialMixApiKey + key → 两条；④ `aggregate` → Skipped；⑤ gateway.json 独有 provider 追加、与 settings 同 id 去重（settings 赢，同用 `relay:<id>` 命名空间）；⑥ `claude-providers.json` → claude API 条目（ANTHROPIC_AUTH_TOKEN/BASE_URL/角色模型）；⑦ 回环 baseUrl 无 upstream → `upstream_missing`；⑧ `CODEX_PLUS_STATE_DIR` 与 `settings.path` 指针生效；⑨ >2000 条报 `external_import.too_many_items`。
- [ ] **Step 2:** `cargo test --manifest-path native/Cargo.toml codex_plus` → FAIL（模块不存在）。
- [ ] **Step 3:** 实现 `resolve_source_dir` / `prepare_entries`（文件缺失视为零贡献，三文件全缺报 `external_import.source_not_found`；JSON 解析失败报 `external_import.source_invalid_json` 带 path 细节；只读打开）。
- [ ] **Step 4:** 同上命令 → PASS；`cargo test --manifest-path native/Cargo.toml cc_switch external_client` → PASS。
- [ ] **Step 5:** `git add native/src/services/codex_plus_import_service.rs native/src/services/mod.rs && git commit -m "新增 Codex++ 配置读取 reader"`

### Task 5: magpie reader

**Files:**
- Create: `native/src/services/magpie_import_service.rs`（`mod.rs` 注册）

**Interfaces:**
- Consumes: `parse_cpa_text`、`PreparedExternalEntry`、`EXTERNAL_CLIENT_MAX_ITEMS`。
- Produces: `pub fn resolve_source_dir(source_path: Option<&str>) -> Result<PathBuf, AppError>`（`XDG_CONFIG_HOME/magpie`，缺省 `~/.config/magpie`）与 `pub async fn prepare_entries(dir: &Path) -> Result<Vec<PreparedExternalEntry>, AppError>`。
- 映射规则：`providers.json` `{providers:[...]}`；每个 Provider 按设置了 URL 的协议逐条产出 `provider:<id>:<protocol>`，`anthropic`→`(Claude, Anthropic)`、`responses`→`(Codex, OpenAiResponses)`、`chat`→`(Codex, OpenAi)`、`gemini`→`(Gemini, Gemini)`；key 链 `key` → `keys[]` 首个有效项（KeyAccount 字段名实现时对 magpie `internal/provider/provider.go` 核对），`keyProtocol` 声明仅适用单协议时其余协议条目 `Broken{"external_import.api_key_missing"}`；无 key → 同 code 的 `Broken`；无任何协议 URL 或插件/远程设备标记（实现时核对 `provider.go` 的标记字段，本计划锁定行为：跳过并计数）→ `Skipped(Unsupported(<标记或"无协议地址">))`；`headers` → `extra_headers`。
- `logins.json`（savedLogin 数组）：`agent:"codex"` → 平铺 `auth.tokens` + `"type":"codex"`、email=`user` → `login:<id|agent:user>`；`agent:"claude"` 且 auth 含 `claudeAiOauth` → 平铺 `{type:"claude", email:<user>, accessToken→access_token, refreshToken→refresh_token, expiresAt→expired}`；auth 为 `{setupToken}` → `{type:"claude", access_token:<setupToken>}`；其余 agent → `Skipped(Unsupported(<agent>))`；`accessOnly` → 显示名追加 ` (access-only)`；`parse_cpa_text` 失败 → `Broken{"external_import.official_snapshot_invalid"}`。

- [ ] **Step 1: 写失败测试**：① 多协议 Provider 拆分（anthropic+chat 两条、id 各带协议后缀）；② 单协议 id 恒带后缀；③ keyProtocol 限定协议；④ 无 key → Broken；⑤ logins codex 条目归一 + platform=codex + email；⑥ claudeAiOauth 条目归一 + platform=claude；⑦ setupToken 条目；⑧ copilot agent → Skipped(Unsupported)；⑨ accessOnly 显示名；⑩ XDG_CONFIG_HOME 生效；⑪ >2000 条报错；⑫ providers.json 与 logins.json 任一存在即可、全缺报 source_not_found。
- [ ] **Step 2:** `cargo test --manifest-path native/Cargo.toml magpie` → FAIL。
- [ ] **Step 3:** 实现（`logins.json` 顶层是数组；`providers.json` 顶层对象含 `providers`）。
- [ ] **Step 4:** 同上命令 → PASS；cc_switch / external_client 回归 → PASS。
- [ ] **Step 5:** `git commit -m "新增 Magpie 配置与订阅账号读取 reader"`

### Task 6: 服务分发与 official 写入路径

**Files:**
- Modify: `native/src/services/external_client_import_service.rs`

**Interfaces:**
- Consumes: Task 4/5 的 `prepare_entries`/`resolve_source_dir`；Task 3 的 `resolve_entries` 分发。
- Produces: preview `items[].kind` 正确填充；official 条目 preview 行 `api_key_masked = mask(access_token)`（从 secret payload JSON 取 `access_token`，缺失用 id_token/setup token 回退，全无则 None）、`base_url/interface_format = None`、`model_mapping_count = 0`；import 落库：official → `create_tx_with_external_source(platform=cred.platform, "official", display_name, email=cred.email, "ok", None, secret, config, preview, Some(ExternalSourceRef))`，preview_json = `RoutePreviewService::generate(platform, "official", ...)`；覆盖走 `overwrite_from_external_source`，`overwrite_target` 加同 kind 校验（`matched.kind` 与条目 kind 不符 → 冲突错误，与现 CONFLICTING_LOCAL_ACCOUNT 同路径）。`counts.unsupported/unsupported_counts` 换真实现（替换 Task 3 的临时键），`counts.official` 统计 official 行。

- [ ] **Step 1: 写失败测试**：① codex-plus fixture 预览：official 行 kind=official、disposition=create、token 打码（序列化结果 `!contains(明文)`）；② magpie logins fixture 预览：claude 条目在 claude 平台列出、codex 条目在 codex 平台列出、互为 other_platform 计数；③ 同源 official 重复导入 → overwrite 不重复（沿用现有 round-trip 写法）；④ 同 source_id 先导 api 再导 official → 冲突错误 `conflicting_local_account`（反之亦然）；⑤ import 落库断言：kind=="official"、secret payload 键集 `{access_token, refresh_token, id_token, account_id, ...}` 与批量导入一致、email 带邮箱、**payload 值等于快照原文**（无刷新）；⑥ `counts.unsupported_counts` 汇总 aggregate/copilot；⑦ 显式传目录 path 的 preview/import 与缺省路径等价。
- [ ] **Step 2:** `cargo test --manifest-path native/Cargo.toml external_client` → FAIL。
- [ ] **Step 3:** 实现（overwrite_target 签名加 `entry_kind: &str` 参数；preview 官方行构建函数单独提取）。
- [ ] **Step 4:** 同上命令 → PASS；`cargo test --manifest-path native/Cargo.toml`（全量 native 测试）→ PASS。
- [ ] **Step 5:** `git commit -m "外部导入支持官方订阅账号写入与按来源分发"`

### Task 7: 前端类型与导入面板

**Files:**
- Modify: `src/lib/api/types.ts`（`ExternalImportClient` union、preview item/counts 新字段）
- Modify: `src/components/accounts/ExternalClientImportPanel.tsx`
- Create: `tests/ExternalClientImportPanel.test.tsx`

**Interfaces:**
- Produces: `EXTERNAL_IMPORT_CLIENT_LABELS` 补 `"codex-plus": "Codex++"`、`"magpie": "Magpie"`；新导出 `EXTERNAL_IMPORT_CLIENT_HINTS: Record<ExternalImportClient, string>`（cc-switch 现文案；codex-plus：`默认读取 ~/.codex-session-delete/ 下的 settings.json、gateway.json 与 claude-providers.json`；magpie：`默认读取 ~/.config/magpie/ 下的 providers.json 与 logins.json`）；`EXTERNAL_IMPORT_CLIENT_PATH_MODE: Record<ExternalImportClient, "file" | "directory">`。

- [ ] **Step 1: 写失败测试**（vitest，仿 `tests/ImportPanel.test.tsx` 模式直接渲染组件）：① labels/hints 按 client 渲染；② `kind:"official"` 行渲染「订阅」徽标，`"api"` 行渲染「API」；③ issue 文案表含 `external_import.upstream_missing`→「缺少上游地址（只有 Codex++ 本机代理地址）」、`external_import.official_snapshot_missing`→「缺少官方登录凭据快照」、`external_import.official_snapshot_invalid`→「官方登录凭据快照无法解析」；④ counts 行渲染「订阅 N」与 unsupported 汇总「另有 N 项暂不支持（aggregate 2）」；⑤ 选择按钮文案按 PATH_MODE 为「选择目录」/「选择文件」。
- [ ] **Step 2:** `pnpm vitest run tests/ExternalClientImportPanel.test.tsx` → FAIL。
- [ ] **Step 3:** 实现类型与面板改动（徽标复用 disposition 徽标样式；unsupported 汇总行样式沿用 other_platform 提示行）。
- [ ] **Step 4:** 同上命令 → PASS；`pnpm typecheck` → PASS。
- [ ] **Step 5:** `git commit -m "导入面板支持 Codex++ 与 Magpie 来源展示"`

### Task 8: AccountsScreen 来源切换与目录选择

**Files:**
- Modify: `src/screens/AccountsScreen.tsx`
- Modify: `tests/AccountsScreen.test.tsx`

**Interfaces:**
- Consumes: Task 7 的 LABELS/HINTS/PATH_MODE。

- [ ] **Step 1: 写失败测试**（仿该文件既有 mock 模式）：① external 模式渲染三个来源按钮；② 切换到 codex-plus → 面板收到 `client="codex-plus"` 且已选集合与自定义路径被清空（mock preview 返回可断言）；③ PATH_MODE=directory 的 client 下点选择按钮调用 `open({directory:true, ...})`（mock `@tauri-apps/plugin-dialog`）；④ cc-switch 仍走文件 filters `["db","json"]`。
- [ ] **Step 2:** `pnpm vitest run tests/AccountsScreen.test.tsx` → FAIL。
- [ ] **Step 3:** 实现：`createMode === "external"` 时在面板上方渲染三分段控件（样式仿既有 createMode 分段按钮）；`chooseExternalClientSource` 按 `EXTERNAL_IMPORT_CLIENT_PATH_MODE[externalClient]` 分支（directory 时 `open({ directory: true, title: "选择 ... 配置目录" })`，无 filters）；来源切换 `setExternalClient` + 清空 `externalSelectedIds`/`externalSourcePath`。
- [ ] **Step 4:** 同上命令 → PASS；`pnpm typecheck` → PASS。
- [ ] **Step 5:** `git commit -m "账号页导入来源切换与目录选择"`

### Task 9: 全量回归验证

**Files:** 无新改动（除非回归修复）。

- [ ] **Step 1:** `pnpm typecheck` → 无错误。
- [ ] **Step 2:** `pnpm test:run` → 全部 PASS。
- [ ] **Step 3:** `cargo test --manifest-path native/Cargo.toml` → 全部 PASS。
- [ ] **Step 4:** 如有回归修复，逐项提交并注明原因；无修复则跳过提交。
- [ ] **Step 5:** 在交付说明中列出：跑过的命令、native/前端测试计数、未在本机验证的范围（Codex++ / Magpie 真实安装样本未接入，fixture 来自上游仓库源码形状）。
