# AGY BYOK

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Status: Prototype](https://img.shields.io/badge/status-prototype-orange.svg)](#当前状态)
[![Platform: macOS](https://img.shields.io/badge/platform-macOS-lightgrey.svg)](#环境要求)

AGY BYOK 是一个面向 **Antigravity IDE、Antigravity App 和 Antigravity CLI** 的本地自定义模型接入工具。

它让你可以在不修改厂商 App Bundle 的前提下，把自己的 API Key 和上游模型接入 Antigravity，并通过本地代理完成协议转换、模型路由和请求转发。

## 它解决什么问题

Antigravity 默认主要使用官方 Cloud Code 服务。如果你希望使用自己的 API Key、公司内部网关、本地模型或其他模型平台，通常需要同时处理三件事：

1. 配置上游服务和模型；
2. 把自定义模型暴露给 Antigravity；
3. 让请求按照不同 Provider 的协议发送，并且能够随时恢复官方配置。

AGY BYOK 把这些操作集中到一个桌面 App 中：

- 在“模型管理”中配置 Provider、API Key 和模型；
- 在“运行概览”中启动本地代理；
- 对 IDE、App 或 CLI 一键启用代理模式；
- 使用完成后，一键恢复对应入口的官方模式。

AGY BYOK 是**本地桌面工具 + 本地 Loopback 代理**，不是 HTTPS 劫持工具，也不会安装根证书、复制应用、修改厂商 Bundle 或重签应用。

## 适合谁

- 想在 Antigravity 中使用 OpenAI、Anthropic、Gemini 或兼容网关的开发者；
- 需要接入公司内部模型网关或本地模型服务的团队；
- 希望同时为 Antigravity IDE、独立 App 和 CLI 配置同一套自定义模型的用户；
- 需要保留官方模式，并能够在自定义模型和官方服务之间快速切换的用户。

## 支持范围

### 接入入口

| 入口 | 接入方式 | 生效方式 |
| :--- | :--- | :--- |
| Antigravity IDE | 修改用户 `settings.json` 中的 `jetski.cloudCodeUrl` | IDE 运行中切换时会按需重启 |
| Antigravity App | 管理 `language_server` wrapper | App 运行中切换时会按需重启 |
| Antigravity CLI | 写入 Shell 环境变量 `CLOUD_CODE_URL` | 新终端或重新加载 Shell 后生效 |

### 上游协议

| 协议 | 常见接口 | 适用场景 |
| :--- | :--- | :--- |
| OpenAI Chat Completions | `/v1/chat/completions` | 大多数 OpenAI 兼容网关 |
| OpenAI Responses | `/v1/responses` | Responses API 兼容服务 |
| Anthropic Messages | `/v1/messages` | Anthropic 官方或兼容服务 |
| Gemini `generateContent` | `/v1beta/models/{model}:generateContent` | Gemini 原生 API |

当前支持文本、内联图片、工具调用、流式响应，以及不同 Provider 的 Thinking / Reasoning 等级映射。

## 如何使用

### 1. 启动桌面 App

当前项目以 macOS 源码构建为主，先安装依赖：

```bash
npm install
npm run tauri dev
```

启动后打开“模型管理”。

### 2. 添加上游服务和模型

在“模型管理”中：

1. 点击“添加上游服务”；
2. 选择协议或使用快捷预设；
3. 填写 API 地址和 API Key；无鉴权的本地服务可以留空 API Key；
4. 点击“获取模型列表”；
5. 勾选需要提供给 Antigravity 使用的模型；
6. 按需确认图像输入、工具调用和 Thinking / Reasoning 能力；
7. 点击“保存上游服务”。

保存后，AGY BYOK 会为选中的上游模型生成宿主可见的 VirtualModel。模型管理页也支持对单个模型或一组模型进行连接测试。

### 3. 启动本地代理

回到“运行概览”，点击“启动代理”。

代理启动前至少需要保存一个可用模型。默认监听地址是：

```text
http://127.0.0.1:54321
```

如果默认端口被占用，桌面端会选择一个空闲的 Loopback 端口，并把实际端口用于后续宿主接入。

### 4. 接入 IDE、App 或 CLI

在“运行概览”对应的入口卡片中点击“启用代理模式”。

- IDE：AGY BYOK 只管理 `jetski.cloudCodeUrl`，不会修改其他 IDE 设置；
- App：AGY BYOK 通过受管理的 Language Server wrapper 指向本地代理；
- CLI：AGY BYOK 向 Shell 配置写入 `CLOUD_CODE_URL`，请打开新终端或重新加载 Shell。

如果对应应用正在运行，IDE 和 App 可能会自动重启以应用新配置。

### 5. 开始使用自定义模型

接入成功后，在 Antigravity 的模型选择器中选择 AGY BYOK 注入的模型即可。

请求会按以下规则处理：

- 官方原生模型继续转发到官方 Cloud Code；
- 自定义 VirtualModel 进入 AGY BYOK 的协议转换和 Provider 路由；
- 只有显式配置 fallback 时，主路由失败后才会尝试一次备用模型；
- 调用日志只记录路由、Provider、状态、耗时和脱敏诊断信息，不记录 Prompt 或回答内容。

### 6. 恢复官方模式

在对应入口卡片中点击“恢复官方模式”。

恢复操作只撤销 AGY BYOK 自己接管的配置，不会删除 Provider、模型或本地配置，也不会自动停止代理服务。

## 配置文件与端口

默认配置文件：

```text
~/Library/Application Support/AGY BYOK/config.v1.json
```

开发环境可以使用 `AGY_BYOK_CONFIG_PATH` 覆盖配置路径。

端口规则：

- 桌面端默认端口为 `54321`；启动时优先使用配置端口；
- 桌面端端口冲突时自动选择空闲 Loopback 端口，并持久化实际端口；
- `cargo run -p agy-byok` 启动的独立代理固定使用 `127.0.0.1:54321`，端口占用时不会自动回退；
- 桌面端设置中的端口变更由后端事务统一处理：运行中会先验证新端口并完成替换，失败时保留旧代理和旧配置；
- 代理只绑定 `127.0.0.1`，不应改为非回环地址作为共享服务使用。

## 安全与隐私

- 厂商 App Bundle 始终只读，不执行 Bundle 修改、codesign 或 quarantine 写入；
- Provider API Key 当前以明文保存在本地配置文件，界面中的密码遮挡不等同于加密存储；
- 代理只监听本机 Loopback；为兼容当前 IDE，部分宿主路由默认不要求 AGY BYOK Token，CORS 策略也尚未收紧；
- 调用日志最多保留最近 200 条内存元数据，不记录 Prompt、回答、Tool 参数、Header 或 API Key；
- 当前安全模型是“本机调用方可信”，不是浏览器 Origin 隔离，也不是多用户共享代理。

## 当前状态

已经可用：

- Provider、上游模型和 VirtualModel 管理；
- 四种上游协议的非流式请求和流式响应；
- 文本、图片、工具调用和 Thinking / Reasoning 映射；
- IDE、App、CLI 的检测、代理模式启用和官方模式恢复；
- 本地配置、模型连接测试和内存调用日志；
- 主题切换和多语言界面。

仍在收口：

- 宿主 Tool Result 与真实多轮 Tool Call ID 的 Fixture；
- `extra_body` 的完整字段校验；
- Host 路由认证和更严格的 CORS 策略；
- API Key 的系统钥匙串或独立 Secret Store；
- 代理自动启动、崩溃恢复和持续健康监控；
- macOS 签名、Notarization 和自动更新。

## 环境要求

- macOS
- Rust stable 与 Cargo
- Node.js 与 npm
- Xcode Command Line Tools

## 开发与构建

安装依赖：

```bash
npm install
```

启动开发版：

```bash
npm run tauri dev
```

构建调试版 macOS App：

```bash
npm run tauri build -- --debug
open "target/debug/bundle/macos/AGY BYOK.app"
```

验证代码：

```bash
npm run build
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

以上验证命令均应在提交前执行；当前工作区已确认 `npm run build`、`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --locked -- -D warnings` 和 `cargo test --workspace --locked` 通过。

## 项目结构

```text
agy-byok/
├── crates/
│   ├── proxy-core/              # 配置、路由、Provider Adapter、HTTP/SSE 代理
│   │   └── src/proxy/           # HTTP 生命周期、路由转发、生成和执行模块
│   └── host-integration/        # IDE settings、App wrapper、CLI Shell 接入
│       └── src/*_integration/   # 发现、所有权、补丁和事务模块
├── src-tauri/                   # Tauri 状态、Commands 和宿主控制
├── src/                         # 原生 TypeScript UI
│   ├── components/              # 页面组件和交互绑定
│   ├── controllers/             # 组件与 Tauri Service 之间的用例边界
│   ├── features/providers/      # Provider 表单、目录、测试和保存逻辑
│   ├── services/                # Tauri invoke 封装
│   ├── store/                   # 前端运行时状态
│   ├── types/                   # 前端类型
│   └── i18n/                    # 多语言资源
├── Cargo.toml
├── package.json
└── README.md
```

## 非官方声明

AGY BYOK 是独立开发的非官方兼容工具，与 Google 或 Antigravity 官方没有隶属、授权或背书关系。项目不会分发 Antigravity 原始二进制或完整源码。

## 许可证

本项目使用 [MIT License](LICENSE)。
