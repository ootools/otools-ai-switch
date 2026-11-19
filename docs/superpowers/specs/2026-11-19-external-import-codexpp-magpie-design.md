# 外部客户端导入扩展：Codex++ 与 Magpie 设计

日期：2026-11-19
状态：待批准

## 意图

「导入其他客户端」目前只支持 CC Switch，且只能导 API Key 型账号。本次扩展把
**Codex++**（[BigPizzaV3/CodexPlusPlus](https://github.com/BigPizzaV3/CodexPlusPlus)）
和 **Magpie**（[yetone/magpie](https://github.com/yetone/magpie)）加入为导入来源，
并让外部导入学会写**官方订阅（OAuth）账号**：Magpie 的订阅账号与 Codex++ 官方
中转的 auth 快照都能以官方账号身份落库，直接接入 AI Switch 现有的官方账号刷新
机制。

动机：Codex++ 与 Magpie 是中文社区当前最主流的两个 Codex/Claude 账号管理工具
（CC Switch 之外），用户在两者里积累了大量中转 API 供应商和订阅账号；一键读入
避免手工搬运密钥。

## 目标 / 非目标 / 反目标

**目标**

1. `codex-plus`、`magpie` 两个新来源出现在「导入其他客户端」，与 cc-switch 并列，
   可自动发现各自默认配置位置，也可手动指定目录。
2. API 型供应商照旧导入为 `kind: "api"` 账号；订阅/OAuth 条目导入为
   `kind: "official"` 账号，落库形状与批量导入一致（同一 secret payload 键集，
   复用现有官方账号刷新机制）。
3. 预览、勾选、按来源记录去重覆盖（`(external_source_client, external_source_id)`
   唯一索引）的行为与 cc-switch 导入完全一致；预览同样全程脱敏。
4. 平台归属按条推断（协议/凭据形状 → 厂商），不需要用户选平台，与批量导入的
   既有原则一致。

**非目标**

- 不写回来源工具，不做持续同步/定时重导；每次导入都是显式动作。
- 不导入 Magpie 插件账号、网关远程账号、Copilot/Gemini/Antigravity 等
  AI Switch 不承载的厂商订阅。
- 不导入 Codex++ 的聚合供应商（aggregate relayMode，它是 Codex++ 内部组合语义）。
- 不在导入时刷新 OAuth token（见反目标）。

**反目标（做错了算失败的事）**

- **导入时向厂商 token 端点发起刷新**。Magpie 自身导入文件时会先刷新一次，
  让来源工具下线；AI Switch 绝不这么做——只存快照。若同一账号仍在 Magpie /
  Codex++ 里被自动刷新，轮换后的 refresh token 会让 AI Switch 的副本失效，
  这是共享登录的固有代价，在 UI 文案中说明即可，不能用「导入时刷新」规避
  （那会直接把用户从来源工具挤下线）。
- 预览载荷泄露明文密钥。

## 约束

- `external_source_client` 的取值一经发布不可改值（孤儿化既有绑定，重导会重复）。
  常量定为 `codex-plus` 与 `magpie`。
- 预览/导入沿用 `EXTERNAL_CLIENT_MAX_ITEMS`（2000）上限。
- 读外部文件必须只读、原子读取（不复用打开中的 SQLite 连接；本次两个来源均为
  JSON 文件，直接读文本）。
- 预览内密钥一律 `mask_api_key` 脱敏。
- 统一账号池：存储平台恒为 codex，逐条 platform 只用于推导 provider。

## 备选方案（已取舍）

1. **方案 A（选定）：扩展外部导入，双 reader + 支持 official 条目。**
   一个入口覆盖两个工具的全部账号类型；预览/去重/覆盖机器全复用；自动发现
   默认路径。代价：外部导入服务要学第二种 kind，改动面最大。
2. 方案 B：OAuth 走批量导入（Magpie 自带导出 sub2api/CLIProxyAPI 文件，批量导入
   今天就能解析；Codex++ 无导出，用户手贴 auth.json），API 部分仍需新 reader。
   被否：UX 分裂、无自动发现、Codex++ OAuth 要手工粘贴。
3. 方案 C：只做 API、OAuth 让用户用 Magpie 导出文件走批量导入。被否：用户明确
   要求 OAuth 也要在应用内直读。

## 数据源与字段映射

### Codex++（reader：`codex_plus_import_service.rs`）

状态目录：`~/.codex-session-delete/`（环境变量 `CODEX_PLUS_STATE_DIR` 覆盖整体
状态目录；`~/settings.path` 指针文件存在且为绝对路径时，settings.json 以指针
所指位置为准）。

读取三个文件，合并为一个条目序列：

| 文件 | 内容 | 条目来源 id |
| --- | --- | --- |
| `settings.json` | `relayProfiles[]`（直连供应商） | `relay:<id>:api` / `relay:<id>:official` |
| `gateway.json` | `providers[].relay`（网关供应商池） | 同上（与直连同命名空间） |
| `claude-providers.json` | `providers[]`（Claude 工具供应商） | `claude:<id>` |

合并规则：`settings.json` 的 relay 先入；`gateway.json` 里 relay id 与已有条目
重复的跳过（网关池常态是直连的镜像），仅追加网关独有的。文件缺失视为该文件
贡献为零，三文件全缺才报「找不到配置」。

**来源 id 必须稳定**：id 全程带 kind 后缀（API 恒为 `:api`，官方恒为
`:official`），单协议不省略；relay 在 settings.json 与 gateway.json 之间搬家
（或直连副本被删）时用同一命名空间 `relay:<id>`，前缀不随文件变。这样用户在
来源工具里的普通编辑（增删协议 URL、追加官方登录、换文件）不会让既有绑定
失效、重导变成重复账号。单协议不再有「无后缀」的短形态，协议区分交给 UI 徽标。

**RelayProfile（camelCase JSON）→ API 条目**（`relayMode` = `mixedApi` / `pureApi`）：

- `interface_format`：`protocol` 映射——`responses` → OpenAI Responses，
  `chatCompletions` → OpenAI Chat，`anthropicMessages` → Anthropic Messages。
- `base_url`：`upstreamBaseUrl` 非空用之，否则 `baseUrl`；两者皆空，或解析出
  指向本机回环（Codex++ 自身协议代理默认地址）且无 upstream 的，报 issue
  `external_import.upstream_missing`（无法导入：它只是个指向 Codex++ 代理的壳）。
- `api_key`：活动 API key（优先 `apiKeys[]` 的 active 项），再用兼容旧 schema 的
  `authContents.OPENAI_API_KEY` 或活动 `[model_providers.*].experimental_bearer_token`
  fallback，最后回退 `apiKey`。绝不把用于访问本机代理的 no-auth key 当成上游 key；
  无上游 key 报 `external_import.api_key_missing`。
- 模型映射：`modelList`（换行分隔的模型 id 列表）逐行 `from == to` 透传，
  与 cc-switch Codex 提取同法。
- `userAgent` 与 `customHeaders[]`（`{key,value}`）→ config `headers`。
- `configContents` 只按 TOML 提取活动 provider 的 `base_url`、`model_provider`、
  `model` 和 `wire_api`/bearer-token fallback；若 `base_url` 指向 Codex++ 管理的
  本地协议代理，再用其专用 `upstreamBaseUrl` / `baseUrl` 字段寻找真实上游；否则
  不从任意 loopback 地址臆测上游。其它 Codex CLI 配置键不导入。

**RelayProfile（`relayMode` = `official`）→ 官方条目（platform codex）**：

- `authContents` 非空：按 Codex CLI `auth.json` 形状输入既有严格
  `parse_auth_json` validator（必须是 `auth_mode=chatgpt`，且包含完整 token、账号与
  client identity）；从成功解析结果保存 email、access/id/refresh tokens、account ID、
  client ID、subscription tier、expiry 与 refresh endpoint。只保留解析器构造的
  payload/config，不留原始 auth JSON。无效、API-key 模式或仅 token 列表的快照显示
  为 `official_snapshot_invalid`。
- `authContents` 为空但 `officialMixApiKey=true` 且有 key：按 API 条目处理（上面
  规则）。否则尝试跨文件解析：`officialAccountRef` 指向的 relay 若在 `gateway.json`
  里且其 `authContents` 非空，以该快照为准（来源 id 仍用 settings relay 的
  `relay:<id>:official`）；解析不到（引用悬空、目标同样无快照）才报 issue
  `external_import.official_snapshot_missing`。
- `officialMixApiKey=true` 且有 authContents 且有 key：产出两条（`relay:<id>:official`
  与 `relay:<id>:api`），让用户各取所需。
- `relayMode` = `aggregate`：跳过，计入 unsupported，不进列表。

**Claude 工具供应商（`claude-providers.json`）→ API 条目（platform claude）**：

`settingsConfig` 与 Claude Code `settings.json` 同形（`{env:{...}}`），提取逻辑
与 cc-switch 的 claude 提取共用（`ANTHROPIC_AUTH_TOKEN`/`ANTHROPIC_API_KEY`/
`ANTHROPIC_BASE_URL`/角色模型 env → 别名映射；1M 后缀规则相同）。`ccsSourceId`
字段忽略——来源去重只认 AI Switch 自己的 `(codex-plus, claude:<id>)` 键。

### Magpie（reader：`magpie_import_service.rs`）

配置目录：`~/.config/magpie/`（`XDG_CONFIG_HOME/magpie`；Magpie 全平台统一走
`~/.config/magpie`）。读两个文件，任一存在即可；全缺报「找不到配置」。

**`providers.json`（`{providers:[Provider...]}`）→ API 条目**：

- `providers.json` 顶层为 `{providers:[...]}`。一个 Provider 的真实形状包括 `id` / `name`、顶层 `key` 与 `keyProtocol`、`keys[]`（各项用 `key` / `protocol` / `off`）、`chat` / `responses` / `anthropic` / `gemini` URL 字段、`models[]`、`headers{}` 和可选 `preset`。
- 一个 Provider 可同时配置多个协议 URL。每个**设置了 URL 的协议**产出一条：
  source_id `provider:<id>:<protocol>`（恒带协议后缀，增删协议 URL 不换 id）。
- 跳过规则：没有任何协议 URL、`preset="remote-magpie"`（当前配置指向另一台机器的 Magpie 网关）的条目计入 unsupported 汇总；无协议 URL 统一标签 `无协议地址`，远端供应商标签 `remote-magpie`。
- 协议 → interface_format / 平台：`anthropic` → Anthropic / claude；
  `responses` → OpenAI Responses / codex；`chat` → OpenAI Chat / codex；
  `gemini` → Gemini / gemini。
- `base_url`：对应协议的 URL 字段（Magpie 约定 chat/responses 以 `/v1` 结尾、
  anthropic 为根地址，原样使用）；`baseAPI` 字段仅供参考，不改变按 URL 拆分。
- `api_key`：`key`，为空取 `keys[]` 首个有效项（实现时核对 KeyAccount 的 JSON
  字段名）；`keyProtocol` 声明首个 key 只适用于某协议时，只对匹配协议的条目生效，
  其余协议条目报 `external_import.api_key_missing`。
- `headers` → config `headers`；模型清单字段透传为 `from == to` 映射（实现时核对
  providers.json 里模型清单的具体字段名）。

**`logins.json`（savedLogin 数组）→ 官方条目**：

- `agent` = `codex`：`auth` 字段为 Codex `auth.json` 原文，归一为
  `{type:"codex", ...auth}` 交官方解析；显示名 = `user`（邮箱），
  email = `user`。
- `agent` = `claude`：`auth` 字段为 `{claudeAiOauth:{...}}` 原生形状，CPA 解析器
  已原生支持并保存 scopes、过期时间、plan、rate limit tier 与 client metadata；
  `profile`（oauthAccount）仅用于补充显示名。Magpie 交由 Claude Code 更新此凭据，
  AI Switch 不猜测另一个 Claude OAuth endpoint/client。
- `agent` = `claude` 且 `auth` 为 `{setupToken:...}`：忽略而不列为可导入官方 OAuth
  account（仅限 Claude inference，不能代表可刷新的 Claude Code sign-in）。
- 其余 agent（copilot / gemini / antigravity / factory / 插件等）：计入
  unsupported，与 Codex++ 的 aggregate 同法展示汇总数量。
- `accessOnly=true` 的 Codex 条目允许以 access/id token 输入官方 parser；如缺少
  refresh token，过期后将不能续期，显示名追加「(access-only)」。
- `lapsed` / `refused`：照常导入，预览不额外拦截——AI Switch 使用时自会暴露，
  不替用户做二次判断。
- source_id 用 savedLogin 的 `id` 字段，缺失时回退 `<agent>:<user>`。

## 服务层改动（`external_client_import_service.rs`）

1. `supported_clients()` → `["cc-switch", "codex-plus", "magpie"]`；校验放行。
2. reader 分发：为引入统一的 reader 接口，把「定位文件 → 读条目 → 提取」抽象为
   每个 client 一个模块三元组（`resolve_source_path` / `read_items` /
   `extract`），`cc_switch_import_service.rs` 保持现状适配进该接口；两个新 reader
   直接产出统一条目。条目结构泛化：现有 `ExternalClientProvider`（cc-switch
   形状）之外，新增 reader 产出的条目带**预提取结果**（API 字段组或官方解析产物
   `ParsedOfficialCredential`），避免把两种 kind 塞进 cc-switch 的
   `settings_config` 假形状里。
3. `classify` 泛化：按条目 kind 分类。API 条目沿用现有平台过滤与 disposition
   逻辑；官方条目新增同等逻辑（平台取解析产物的 platform）。
4. `overwrite_target` 放宽为**同 kind** 才允许覆盖：`(client, source_id)` 命中的
   已有行，kind 不同（api ↔ official）→ 报
   `external_import.conflicting_local_account`；kind 相同 → 照旧覆盖。
5. 写入：API 条目走现有 `create_tx_with_external_source` 路径；官方条目用
   `ParsedOfficialCredential` 的三件 JSON（与批量导入 `create_batch_credential`
   同构），经 `create_tx_with_external_source` 落库（复用同一外部来源绑定），
   `email` 带上登录邮箱。
6. 预览模型加 `kind` 字段（`api` / `official`）与 per-kind 计数；前端列表按 kind
   展示徽标。载荷形状变化是**加字段**，不破坏 cc-switch 既有前端行为。

## 前端改动

- `ExternalImportClient` 类型扩为 `"cc-switch" | "codex-plus" | "magpie"`；
  `EXTERNAL_IMPORT_CLIENT_LABELS` 补 Codex++ / Magpie 标签。
- 「手动指定位置」对两个新来源是**选目录**（状态目录 / 配置目录），cc-switch
  保持选文件不变：`source_path` 对新来源按目录校验（不存在报
  `external_import.source_missing`，code 复用、语义放宽），预览 `source_path`
  显示所选目录；目录内的 `settings.json` 以所选目录为准，指针文件只在走默认
  路径时生效。选文件推导兄弟文件不可取（`settings.path` 重定向时
  settings.json 不在兄弟位置）。
- `AccountsScreen`「导入其他客户端」模式加来源切换（三分段控件），切换时清空
  已选与自定义路径；查询键已含 client，天然隔离。
- `ExternalClientImportPanel` 文案参数化：各来源一句话提示默认位置
  （Codex++ `~/.codex-session-delete/`；Magpie `~/.config/magpie/`），条目行加
  kind 徽标（API / 订阅），issue 文案表补新 code。

## 测试要求

- reader 单测：两个工具的真实形状 fixture（camelCase RelayProfile、多协议
  Provider、`logins.json` 各 agent 条目、setup-token、access-only、缺失字段）；
  默认路径解析（`CODEX_PLUS_STATE_DIR`、`settings.path` 指针、`XDG_CONFIG_HOME`）。
- 服务层：预览计数（api/official/unsupported）、同 kind 覆盖、跨 kind 冲突报
  issue、重导覆盖不重复（沿用 cc-switch 服务测试的写法，SQLite 内存池）。
- 落库形状断言：official 条目 secret payload 键集与批量导入一致；预览序列化
  不含明文密钥。
- 前端：来源切换查询隔离与标签渲染（vitest 既有模式）。

## 留给实现者的决定

- Magpie `keys[]`（KeyAccount）、模型清单、插件/远程设备标记的准确 JSON 名
  （以 magpie 源码 `internal/provider/provider.go` 为准）。
- Codex++ 回环地址判定的精确阈值（默认协议代理端口清单，读到哪个算「壳」）。
- issue code 的最终措辞（新增 `external_import.upstream_missing`、
  `external_import.official_snapshot_missing`，或合并进既有 code）。
- Magpie portable / beside-exe 安装形态是否值得一并发现（建议 v1 只支持
  `~/.config/magpie` + `XDG_CONFIG_HOME`，其余留给手动选目录）。
- 测试样本来源：优先用两个上游仓库自带的测试 fixture 与源码形状合成；若用户
  能提供真实配置文件样本则替换为真实样本。
