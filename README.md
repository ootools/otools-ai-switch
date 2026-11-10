# AI Switch for OTools

AI Switch 的独立 OTools 插件版本，默认以 Panel 打开。管理界面通过 OTools SDK 使用宿主的桌面或远程 Web 通道；本地 AI 协议接口默认地址为 `http://127.0.0.1:10888`。

## 功能范围

保留账号池、凭据导入导出、模型映射与路由、客户端接入及配置快照、用量统计、生图、MCP、Skills 和会话浏览。模型接口保留原项目的文本、图片、音频和视频协议能力。

本版不包含终端 Vibe、独立 Tauri 应用壳、自建管理网页服务、Tailscale、ai-switch 插件机制及 SaaS 运营站点。生图作为普通内置功能提供。关闭管理 Panel 不等于停止 AI 服务；需要停止时使用界面的服务开关，插件卸载/更新时由宿主触发 native 停机。

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
node scripts/pack-plugin.mjs
node scripts/verify-plugin.mjs
```

Native 默认构建 release 库并复制到 `lib/Windows.dll`、`lib/macOS.dylib` 或 `lib/Linux.so`。交叉编译可传 `--target <Rust target>`，需要自行安装对应工具链。烟测默认加载 Windows 库；其他平台通过 `--library lib/Linux.so` 等指定。

`dist-pack/otools-ai-switch-0.1.0.oplg` 是 ZIP 格式的插件安装包，旁边提供 `SHA256SUMS`。打包必须先有前端 `dist/` 和至少一个平台的 native 库；缺少后端时会拒绝生成不能工作的前端空包，并检查入口 HTML 引用的静态资源是否随包提供。

`.github/workflows/build-plugin.yml` 提供 Windows x64、Linux x64、macOS ARM64 的分别构建和制品校验配置，不自动发布。每份包只适用于其包含的 native 平台和 CPU 架构。CI 配置存在不代表这些平台已在当前工作区验证。

本机 `sccache` 配置有问题时，可仅对当前命令清空 `RUSTC_WRAPPER` 后重试 Cargo；无需修改全局配置。

## 使用

1. 在 OTools 插件管理中选择本地安装，导入 `.oplg`。
2. 搜索 AI Switch 打开管理 Panel，添加或导入账号。
3. 在账号池开启路由服务，按需使用账号池工具栏写入客户端配置。AI 默认地址为 `http://127.0.0.1:10888`。

10888 被占用时会明确报错，不自动切换到另一个端口。AI 接口需要本地 API Key；管理命令不会暴露在 10888。OTools 远程 Web 运行需要远端宿主安装匹配平台的 native 库。

## 来源和许可

来源版本及 SDK revision 见 [UPSTREAM.md](UPSTREAM.md)。保留上游 MIT 和 Apache-2.0 声明，见 `LICENSE`、`THIRD_PARTY_NOTICES.md` 和 `LICENSES/`。

具体构建、测试与运行验收结果将在交付说明中列出；未在本机执行的平台构建不代表已经验证。
