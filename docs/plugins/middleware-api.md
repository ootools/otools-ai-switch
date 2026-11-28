# Middleware API v1

AI Switch 的内置插件与本地安装的中间件使用相同的 descriptor、JavaScript SDK、阶段和能力检查。`builtin.*` 只表示随应用分发；`pack:<id>` 表示安装包。业务执行器不根据这两种身份授予不同能力。

可安装示例：[`examples/middleware/account-health`](../../examples/middleware/account-health)。示例使用内置账号故障处理的同一份脚本，换了包 ID。通过插件设置中的“本地文件夹安装”导入，再添加到账号的“失败恢复”列表。请先移除该账号原有同功能插件，避免同时配置两套规则。

## 包清单

目录包含 `plugin.json`、`middleware.js`。清单仍使用 `schema_version: 1`；宿主接口版本在 `contributes.middleware.descriptor.api_version` 中声明。descriptor 的 id 最终由宿主规范为 `pack:<清单 id>`，不能冒充内置 ID。

```json
{
  "schema_version": 1,
  "id": "my-account-monitor",
  "version": "1.0.0",
  "name": "我的账号监控",
  "description": "看到特定错误时标记当前账号",
  "author": "me",
  "license": "MIT",
  "contributes": {
    "middleware": {
      "file": "middleware.js",
      "descriptor": {
        "id": "my-account-monitor",
        "name": "我的账号监控",
        "description": "按上游错误标记当前账号",
        "api_version": 1,
        "lane": "recovery",
        "kind": "schedule",
        "stage": "policy",
        "hooks": ["onFailure", "onComplete"],
        "capabilities": ["account.read", "account.write", "state.read", "state.write", "diagnostics.log"],
        "defaults": { "seconds": 30 },
        "config_fields": [{ "key": "seconds", "label": "冷却秒数", "kind": "number", "min": 0, "max": 86400 }]
      }
    }
  }
}
```

`lane` 支持 request/response/recovery/both；`kind` 支持 transform/schedule。`config_fields` 支持 number/bool/select/textarea，以及通用 userAgent（预设＋文本）和 headers（名称／值／操作列表）编辑器。`rule_editor` 可为 retry/stream/health，`class_limits_editor: true` 显示分类次数设置；对第三方与内置均有效。`defaults` 与账号保存的 config 合并后提供为 `ctx.config`。

## 生命周期

| Hook | 输入与用途 | 返回值 |
| --- | --- | --- |
| `onRequestHeaders(event, ctx)` | 请求头阶段，`event.phase` 为 before_auth / after_auth | `{value: {headers: {...}}}`，null 值删除头 |
| `onRequest(body, ctx)` | descriptor 指定阶段的 JSON 出站请求 | 新 body、`host.transform(body, headers)` 或 null |
| `onResponseHeaders(event, ctx)` | `{status, headers}` | `{value: {headers: {...}}}`，null 值删除头 |
| `onResponse(body, ctx)` | 缓冲的 JSON 下游响应 | 新 body、`host.transform(body, headers)` 或 null |
| `onStreamEvent(event, ctx)` | 已分帧的 JSON SSE 事件 | `{value: event}`；按链顺序传给后一个插件 |
| `onFailure(event, ctx)` | 规范化失败事实与 phase | `{actions: [...]}` |
| `onComplete(event, ctx)` | 成功收尾，宿主完成常规健康恢复之后 | `{actions: [...]}` |
| `policy(config, ctx)` | `ctx.domain` 为 request/stream | 有界策略字段或 null |

请求/响应阶段开放给每个插件：`perAttempt`、`inlineRemote`、`imageLimit`、`preBridge`、`forceReasoning`、`postBridgeCross`、`postBridgePlaintext`、`postBridgeEncrypted`、`postBridgeReminder`、`wire`、`retryRecovery`、`response`。这些名字沿用已有处理顺序，不是插件 ID；所有插件都能声明同一阶段。`stages` 可声明多个阶段。

输入使用序列化副本。串流观察、正文缓冲和账户结算的 hook 不能随意互换：这是执行阶段约束，内置插件也受同样约束。图片/音频等非 JSON 正文不会传给 JSON body hook。

## onFailure 的 phase 与动作

| event.phase | 何时调用 | 允许的业务动作 |
| --- | --- | --- |
| `before_output` | 正文提交前，判断是否重试 | retry / next_account / return |
| `request_recovery` | 提交前尝试修正当前失败请求 | request.replace；仅不同 body 才重试，每插件本次调用最多一次 |
| `stream` | 已输出内容后的错误或中断 | continue / return；宿主检查可续写条件与预算 |
| `account_failure` | 确定要结算账号/模型失败 | account.failure；返回结算参数，不在 hook 内写数据库 |
| `account_event` | 某次尝试的失败记录 | account.update、account.success、命名空间状态写入 |

所有阶段都有 `status`（可能为空）、`code`、`type`、`message`、`transport` 和 `terminal`。账号事件提供 `attempt`；`ctx.event_id` 是宿主生成的请求/尝试标识。`ctx.account` 与 `ctx.state` 只在持久动作阶段且声明对应读能力时提供，绝不包含密钥。`request_recovery` 提供 `ctx.request`；流相关阶段提供 `ctx.output_committed`。

不支持的阶段/动作组合会拒绝整组动作并记录原因，不会偷偷忽略某个写入。`onStreamEvent` 用于流内容变换，恢复决定放在 `onFailure`。不要把返回值为 null 理解为停止恢复；明确停止应返回 `host.retry.stop()`。

## 公开 SDK

```javascript
function onFailure(event, ctx) {
  if (event.phase !== "account_event") return null;
  if (event.code !== "my_provider_disabled") return null;
  return { actions: [
    host.accounts.update({status: "error", cooldown_seconds: ctx.config.seconds}),
    host.state.increment("matched_failures", 1),
    host.diagnostics.log("info", "Provider disabled this account")
  ] };
}
```

- `host.accounts.snapshot(ctx)`：读取当前已提供的账号快照。
- `host.accounts.update({status?, cooldown_seconds?})`：更新当前绑定账号；status 只允许 ok/error，不能覆盖 paused/revoked。
- `host.accounts.recordFailure({threshold, error_status_enabled, cooldown_seconds?, rule_id?, rule_name?})`：仅 account_failure 阶段，生成失败结算参数。宿主的账号/模型仓储统一记一次失败；插件不能在 account_event 再重复增加宿主失败计数。自定义计数使用 `host.state.increment`。
- `host.accounts.recordSuccess()`：清理当前账号失败计数与冷却，不覆盖用户暂停/撤销状态。
- `host.retry.request(match)` / `.nextAccount(match)` / `.stop(match)`：提交恢复决定，受策略预算与输出阶段限制。
- `host.streams.continueResponse(match)`：申请续写；宿主实现协议重建、序号/ID 连续性与已输出内容保护。需要 stream 策略预算。
- `host.request.replace(body)`：提交修复后的请求，不能指定其他账号。
- `host.state.get(ctx, key)` / `.set(key, value)` / `.increment(key, amount)`：账号 + 插件命名空间状态，increment 原子执行。
- `host.errors.match(rules, event)`：调用与界面样例测试相同的规则匹配器。
- `host.transform(body, headers)`：body hook 的通用 JSON 与头修改返回值。
- `host.codec.compressBrotli(base64)` / `host.images.fetchAsDataUrl(url)`：共享编解码/图片获取能力，分别需要 codec.brotli/network.image。
- `host.diagnostics.log(level, message)`：构造日志动作。宿主诊断使用 `plugin_middleware_diagnostics` 命令读取，保留最近 256 条。

SDK 动作函数只构造命令，不会立即写数据库。hook 成功返回且全部动作通过校验后才交给执行器。未知动作或未知选项会拒绝整组命令。

## 策略、并发与停用

`policy` 的 request/stream 两个 domain 从各自全关闭基线开始。插件必须区分 domain，避免把普通重试预算带入续写。字段沿用 retry_count/retry_interval_ms/retry_class_limits/failover_enabled 等，返回值同样接受宿主上限校验，不因来自脚本而放宽。

持久写入在单个事务内执行，使用绑定账号、规范化插件 ID、宿主 event_id 去重。插件无法通过传入 account_id/plugin_id 改写其他对象。写入前重读账号中间件列表，已移除/关闭的插件不提交；全局停用与写事务通过生命周期锁排序。已提交事务不因随后停用而撤销。

各插件拥有隔离的 JavaScript Context。脚本加载和调用有时间上限，运行时有内存上限；异常/超时不提交该 hook 的动作。流事件逐个变换，未变化的健康流保留原始字节。网络请求和数据库事务不会作为任意 SQL/fetch 暴露给脚本。

原有无版本账号配置的兼容迁移、协议格式转换、凭据刷新/撤销、终止流保护仍属于宿主职责，不通过插件 ID 授予专属权限。历史诊断计数和请求日志与插件显式状态动作分开。


## 架构 review 修正（2026-11-28）

- 请求正文仅遍历请求链，响应正文仅遍历响应链。响应头/流事件从响应与恢复链分派；同一公共事件按插件 ID 去重。双向插件可以分别添加到两条链，不会因此在一个正文阶段执行两次。
- 首次请求和续写共用输入准备、协议构建后的处理、编码与响应头钩子。各阶段的 body/header 结果统一应用，`ctx.stage` 为真实执行阶段；返回 null 或抛错时保留原正文的字节。
- 失败处理结果用于执行后直接写诊断，生成请求日志不会再次调用失败钩子。账号事件进入当前尝试的队列，宿主失败结算完成后再提交显式插件动作，避免刚设置的冷却被内部清理覆盖。
- 每个插件拥有独立 Runtime/Context 与执行锁，加载后以不可变版本绑定发布。重建串行化并校验 generation；过期构建丢弃，并发懒初始化不会反复重建同一版本。账号事件构造上下文和调用脚本全程使用同一版本。
- 多线程 Tokio 宿主在获取脚本锁和进入阻塞宿主函数前让出 async worker；同插件请求仍按其执行锁顺序运行，其他插件和定时器可以继续。图片 DNS、连接、读取受一个总超时约束，所有解析地址必须是公网地址，连接固定到经过验证的地址，不跟随重定向、不使用环境代理；分块读取中检查 16MiB 上限。
- 停用、卸载或加载失败不会使账号保存丢失插件原配置。安装先校验脚本，再完整暂存目录与设置，切换失败会恢复旧版本；回滚失败保留恢复备份并报告。卸载的设置写入失败也会使已删除插件的旧运行器失效。


## 请求头与图片插件整合（2026-11-28）

- “请求头设置”承接旧高级页的 User-Agent 与 config.headers。请求头规则支持 fill/set/remove，User-Agent 保留默认、浏览器和 CLI 预设。编辑账号从请求处理进入；创建账号的 User-Agent 快捷输入保存为同一插件。
- onRequestHeaders 是所有插件都可声明的通用 hook，使用 request.transform 能力。before_auth 用于填补缺失的导入头；after_auth 收集最终覆盖。显式覆盖/删除在默认客户端身份处理后保留。内置请求头规则禁止覆盖认证头，禁止自定义连接、Content-Encoding 等传输头，避免与 Brotli 冲突。
- “图片处理”合并 inlineRemoteImages 与 imageLimit：远程下载内嵌、旧图数量限制各自独立开关；max_images 默认 10，范围 1–1000。只限制启用后的历史图片数量，不缩放像素或重编码图片。HTTP Brotli 功能改名“请求体压缩”，仍单独配置。
- catalog_revision=1 记录一次性迁移。旧 v1/v2/平铺配置均可升级；合并后保留旧实例配置在 legacy_plugins，已有新插件时只归档旧条目，不重复执行。旧请求直通中的自定义头会单独迁入有效插件，其余原有请求插件保持停用。
- 已迁移账号移除请求头/图片插件后，旧平铺字段不再复活实例。省略空方向的 v2 配置按原生默认补齐，不回退到旧配置。
