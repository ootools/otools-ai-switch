# AI Switch for OTools

AI Switch 的独立 OTools 插件版本，默认以 Panel 打开。窗口尺寸沿用 OTools 的默认或记忆尺寸，插件不设置 `panelWidth` / `panelHeight`。Logo 使用原版 AI Switch 设计。管理界面通过 OTools SDK 使用宿主的桌面或远程 Web 通道；本地 AI 协议接口默认地址为 `http://127.0.0.1:29527`。

通用 TAB 的账号来源筛选会根据当前可用宽度，将放不下的 provider 收到“更多”下拉；窗口变宽后恢复行内显示，并保持已选来源。

## 功能范围

保留账号池、凭据导入导出、模型映射与路由、客户端接入及配置快照、用量统计、生图、MCP、Skills 和会话浏览。模型接口保留原项目的文本、图片、音频和视频协议能力。

本版不包含终端 Vibe、独立 Tauri 应用壳、自建管理网页服务、Tailscale、ai-switch 插件机制及 SaaS 运营站点。生图作为普通内置功能提供。关闭管理 Panel 不等于停止 AI 服务；需要停止时使用界面的服务开关，插件卸载/更新时由宿主触发 native 停机。

桌面 OTools 启动时通过插件 `autostart.tasks` 调用 `start_route_proxy`，不需要打开 AI Switch Panel 就会启动 AI 服务。首次安装后的即时启动可点页面开关，重启宿主后由后台任务启动；主动停止只影响本次运行，下一次宿主启动仍会执行后台任务。端口占用会报告错误，重复启动不会创建额外监听器。纯 CLI 模式目前没有宿主的自动任务执行器，不能将桌面后台启动验证视为 CLI 已自动启动。

图片、视频、音频 TAB 提供各自端点和共享 Key，客户端配置写入和基线模型设置仅在通用 TAB 展示。向量接口仍未开放，不展示可用推理端点。

图片账号新增/编辑使用图片专用协议，提供 OpenAI / GPT Image、Google Nano Banana / Nano Banana Pro 及豆包 Seedream（火山方舟）预设，并显示实际 POST 上游端点。Base URL 填服务根路径而非完整生成路径；模型填写上游开放的图片 Model ID 或 Endpoint ID，保存时会带 `image.generate` 能力。Google 当前为 Gemini `generateContent` 生图，非 Imagen `predict`；OpenAI 预设使用 API Key，不是 ChatGPT 登录令牌。内容过滤、Brotli 请求压缩、文本推理和工具兼容只在通用 TAB 中配置，媒体表单保存会保留已有隐藏设置。

Google 图片预设分为 Nano Banana（`gemini-2.5-flash-image`）与 Nano Banana Pro（`gemini-3-pro-image-preview`），两者使用 Google API Key 和相同根地址 `https://generativelanguage.googleapis.com`，实际请求按模型 ID 路由到对应 `generateContent` 端点。编辑时结合模型映射识别预设，不会因同一 Base URL 把 Pro 错认成初代；模型是否开放仍取决于上游和账号权限。

数据目录为 `~/.otools/local/otools-ai-switch/`，与原版 `~/.ai-switch` 分离。正式版数据库是该目录下的 `ai-switch.db`，调试版为 `ai-switch-dev.db`；设置为 `settings.json`。例如当前 Windows 用户的正式数据库为 `C:\Users\Admin\.otools\local\otools-ai-switch\ai-switch.db`。远程 Web 模式数据保存在运行 OTools 的宿主机上。

迁移账号请使用原版导出和插件导入，不要让两端共写同一 SQLite 数据库。测试可通过 `OTOOLS_AI_SWITCH_DATA_DIR` 指定临时数据目录。左侧不提供独立“客户端接入”入口，账号池内的配置相关业务保留。

## 本地构建

需要 Node.js 22.15+、pnpm、Rust，以及当前平台对应的 C/C++ 编译工具。SDK 源码已固定在 `vendor/otools-plugin-sdk`，构建无需原版 ai-switch 或相邻 OTools 仓库。

```sh
pnpm install --frozen-lockfile
pnpm typecheck
pnpm test:run
pnpm build
node scripts/build-native.mjs
node --test scripts/package.test.mjs
python -I scripts/smoke-native.py
python -I scripts/test_autostart.py
python -I scripts/test_nano_banana.py
node scripts/pack-plugin.mjs
node scripts/verify-plugin.mjs
```

Native 默认构建 release 库并复制到 `lib/Windows.dll`、`lib/macOS.dylib` 或 `lib/Linux.so`。交叉编译可传 `--target <Rust target>`，需要自行安装对应工具链。烟测默认加载 Windows 库；其他平台通过 `--library lib/Linux.so` 等指定。

`dist-pack/otools-ai-switch-0.1.2.oplg` 是 ZIP 格式的插件安装包，旁边提供 `SHA256SUMS`。打包必须先有前端 `dist/` 和至少一个平台的 native 库；缺少后端时会拒绝生成不能工作的前端空包，并检查入口 HTML 引用的静态资源是否随包提供。

分平台包用 `--suffix <platform>` 命名，例如 `node scripts/pack-plugin.mjs --suffix windows` 生成 `otools-ai-switch-0.1.2-windows.oplg`；校验时传同样的 `--suffix`，并可加 `--expect-library lib/Windows.dll` 强制断言包里**只含这一个** native 库。`SHA256SUMS` 会按实际包名生成。市场上的 `packageMode=1` 即依赖这三个单平台包，客户端优先下载平台包、失败才回退不带后缀的合包。

`.github/workflows/build-plugin.yml` 提供 Windows x64、Linux x64、macOS ARM64 的分别构建和制品校验配置，不自动发布。每个 matrix job 只构建自己那一个 native 库，打包时带平台后缀并用 `--expect-library` 阻断多平台库混入。macOS 目前只有 ARM64 构建。CI 配置存在不代表这些平台已在当前工作区验证。

本机 `sccache` 配置有问题时，可仅对当前命令清空 `RUSTC_WRAPPER` 后重试 Cargo；无需修改全局配置。

## 使用

1. 在 OTools 插件管理中选择本地安装，导入 `.oplg`。
2. 搜索 AI Switch 打开管理 Panel，添加或导入账号。
3. 在账号池开启路由服务，按需使用账号池工具栏写入客户端配置。AI 默认地址为 `http://127.0.0.1:29527`。

29527 被占用时会明确报错，不自动切换到另一个端口。AI 接口需要本地 API Key；管理命令不会暴露在 29527。OTools 远程 Web 运行需要远端宿主安装匹配平台的 native 库。

## 来源和许可

来源版本及 SDK revision 见 [UPSTREAM.md](UPSTREAM.md)。保留上游 MIT 和 Apache-2.0 声明，见 `LICENSE`、`THIRD_PARTY_NOTICES.md` 和 `LICENSES/`。

具体构建、测试与运行验收结果将在交付说明中列出；未在本机执行的平台构建不代表已经验证。
