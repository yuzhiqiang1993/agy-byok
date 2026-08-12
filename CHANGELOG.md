# 更新日志

## [1.2.0] - 2026-08-12

### What's New

- **Brand New 3-Step Upstream Service Wizard**: Redesigned the upstream provider onboarding flow into an intuitive 3-step wizard (Select Provider Preset -> Connection & Endpoint Config -> Select & Configure Models). Built-in presets cover major official providers (OpenAI, Anthropic, Gemini, xAI, DeepSeek, etc.), popular aggregators, and local LLMs (Ollama), featuring instant connection testing and fine-grained capability mapping.
- **Manage & Hide Official Native Models**: Added the ability to selectively disable or hide Antigravity's built-in official models in the model management interface, keeping your IDE and App model selector clean and clutter-free.
- **Custom Context Compression Policies**: Support multiple physical capacity tiers (128K, 200K, 256K, 372K, 1M) with visual capacity allocation bars. You can now fine-tune compression triggers, checkpoint limits, and output buffers using percentage shortcuts or exact token counts to avoid frequent, conservative official summaries.
- **Custom Host Installation Paths**: Support manually setting custom installation directories for Antigravity IDE and App, improving host discovery and integration when apps are installed in non-standard locations.
- **Raw Catalog Inspector Panel**: Added raw payload inspection for upstream and official model catalogs to streamline debugging and metadata troubleshooting.

### Improvements & UI Refinements

- **Modern Minimalist UI System Upgrade**: Redesigned model cards with a clean, Vercel-inspired aesthetic, improved contrast and typography hierarchy, and standardized Modal and Notice components across the entire app.
- **Layout & Bottom Bar Fixes**: Refined the fixed header/footer architecture for wizards and settings dialogs, eliminating negative margins and ensuring zero clipping for 58px sticky action bars.
- **Enhanced Overview & Diagnostics**: Improved proxy status awareness, host process detection, and activity log diagnostics. Added confirmation dialogs before restarting client applications.
- **Documentation & Asset Refresh**: Fully updated high-resolution bilingual screenshots and comprehensive guides on context compression optimization.

### Bug Fixes

- **macOS Host Lifecycle Management**: Fixed macOS Antigravity App launch argument ordering and abnormal exit handling; aligned proxy teardown logic between App and CLI.
- **Proxy Timeout & Concurrency**: Enhanced native passthrough timeout controls, fixed Host header destination parsing, and boosted request concurrency handling with queued dispatch.
- **Policy & Worker Constraints**: Fixed compression policy disable logic, default limit synchronization, and custom model worker clamps; resolved full payload display issues in BYOK data modals.

### Build & Release

- Upgraded desktop application version to `1.2.0`.
- Maintained cross-platform builds and auto-updater signatures across macOS (Apple Silicon / Intel) and Windows.


## [1.1.3] - 2026-08-10

### 问题修复

- **官方模型目录跟随实时元数据**：移除固定模型 ID、固定数量和静态旧别名，改用接口返回的 Agent 排序、推荐标记与废弃替换映射。
- **新旧模型策略保持一致**：官方接口标记模型过时后，按返回的映射同时维护新旧模型 ID 的压缩策略，避免配置只落到单一过时 ID。
- **官方模型页面停止重复刷新**：仅在压缩策略确实发生变化时保存配置，避免配置保存触发重复重绘和界面闪烁。

### 构建与发布

- 桌面端版本升级至 `1.1.3`。
- 保持 macOS、Windows 与自动更新产物的既有发布流程。

### 验证

- Rust 工作区测试：204 项通过。
- 前端 TypeScript、Vite 与 534 个 i18n 翻译键校验通过。
- Harness quick 自检通过。
- macOS Apple Silicon 本地 `.app`、`.dmg` 与更新压缩包构建通过。

## [1.1.2] - 2026-08-10

### 破坏性变更

- **配置契约升级**：模型能力现在要求显式保存 `supported_mime_types`，Thinking 配置要求显式保存支持状态、默认预算和最小预算的可空字段。旧版配置不会自动补齐或迁移；升级时请卸载重装并重新配置模型。

### 新增功能

- **完整媒体能力配置**：自定义模型可以保存并展示图片、视频和 MIME 类型能力；Gemini 可原样转发视频及其他已声明的内联数据，非 Gemini 协议会拒绝未验证的媒体格式。
- **Gemini Thinking 预算**：支持模型级 `thinkingBudget`、`minThinkingBudget`、动态预算与关闭预算，并保持请求级推理档位优先于模型默认预算。
- **目录 Checkpointer 导入**：完整且通过校验的官方 Checkpointer 策略可作为新建自定义模型的默认压缩策略，残缺或超出可信模型容量的策略不会导入。
- **宿主安装位置发现**：macOS 可通过应用标识发现移动后的 Antigravity 安装，Windows 可从 App Paths、卸载注册表和常见安装目录定位宿主程序。

### 问题修复

- **流式请求不再受总时长误杀**：请求超时只约束上游响应头等待，已建立的流由逐块空闲超时保护。
- **首帧前错误正确返回**：上游在流开始前失败时返回真实 HTTP 错误；流开始后的错误通过顶层错误帧通知客户端，避免请求卡住。
- **会话模型标识准确**：响应会写入实际上游模型 ID，避免本地会话记录为自定义占位模型。

### 构建与发布

- macOS 手动安装包保留 DMG，同时生成仅供应用自动更新使用的 `.app.tar.gz`；Windows 保留 NSIS 安装包，并使用包含系统与架构的清晰文件名。
- 更新清单统一指向 GitHub Release 的公开下载地址，并覆盖 macOS 与 Windows 平台。

### 验证

- Rust 工作区测试：244 项通过，1 项环境测试按设计忽略。
- 前端 TypeScript、Vite 与 534 个 i18n 翻译键校验通过。
- macOS Apple Silicon DMG 本地构建及镜像校验通过。

## [1.1.1] - 2026-08-10

### 问题修复

- **CLI 接入不再干扰 App**：启用或停用 CLI 代理时只更新用户环境配置，不再停止、启动或重启 Antigravity App。
- **操作提示更明确**：中英文界面均说明 CLI 配置需要完全退出并重新打开终端应用后生效。

### 文档与发布

- 补充模型级上下文压缩配置说明，以及 macOS、Windows 安装包和架构选择说明。
- macOS arm64、macOS x64 和 Windows x64 发布产物与更新清单均通过流水线校验。

## [1.1.0] - 2026-08-09

### 新增功能

- **模型级 Checkpointer 策略**：支持按官方模型和自定义模型配置压缩策略、工作模型与关联参数。
- **官方模型与客户端代理接入**：完善官方模型目录获取，并统一 Antigravity IDE、App 与 CLI 的代理配置入口。
- **模型管理界面重构**：统一官方模型和自定义模型卡片，支持模型维度的压缩策略配置。

### 体验优化

- 修复宿主客户端启动后的按钮状态与国际化同步。
- 优化侧边栏布局、定位与粘性滚动体验。

## [1.0.5] - 2026-08-08

### 稳定性与兼容性

- **修复自定义模型注入后立即失败**：不再向自定义占位模型注入实验性 `CASCADE_USE_EXPERIMENT_CHECKPOINTER`。`v1.0.4` 将占位模型自身写为 `checkpoint_model`，会触发 Antigravity Language Server 的 `bad checkpoint state`，并在上游生成请求发出前终止会话。
- **保留自定义压缩配置**：全局自定义模型策略和模型级 `checkpoint_override` 继续按原结构保存与校验，避免丢失用户配置；在 Antigravity 提供可验证的自定义模型 Checkpoint 契约前，不再把这些配置转换为不稳定的实验字段。
- **保持官方模型行为**：官方 Gemini 与 Claude 的 Checkpoint 覆盖逻辑不变。
- **修复 IDE 代理停用判断**：ownership 文件缺失时，只要 IDE 当前 endpoint 仍指向本代理，也允许安全停用并正确标记 endpoint 匹配状态。

### 验证

- `cargo fmt --all -- --check`
- `cargo check --workspace --all-targets`
- `cargo test --workspace`
- `npm run build`
- `git diff --check`

## [1.0.4] - 2026-08-08

### 新增功能

- **独立压缩策略入口**：将官方 Gemini、Claude 与自定义模型的上下文压缩策略提升为一级导航，集中管理模型目录中的 Checkpoint 参数。
- **侧边栏导航优化**：调整桌面端侧边栏布局与导航顺序，补充导航项无障碍标签，并统一使用跨平台 PNG 应用图标。

### 稳定性与兼容性

- **自定义模型默认保持官方行为**：自定义模型默认不主动注入 Checkpoint 压缩策略；显式的模型级百分比覆盖仍可单独启用。
- **官方模型配置行为明确**：选择 `official` 档位时，统一 schema 中保留的百分比字段仅作为默认占位值，不会覆盖官方模型的上游 Checkpoint 参数。
- **压缩档位切换更安全**：离开 `custom` 档位时恢复默认百分比，避免旧的自定义值在之后重新启用自定义档位时残留。
- **错误信息更可诊断**：保留结构化上游错误详情并限制长度，改善回退失败与活动日志中的错误反馈。
- **应用图标兼容性优化**：统一桌面端和前端图标资源格式，减少不同平台的显示差异。

### 验证

- 前端 `npm run build`：通过。
- `cargo test -p agy-byok checkpoint --lib`：25 项通过。
- `cargo test -p host-integration`：23 项通过，1 项按设计忽略。

## [1.0.3] - 2026-08-08

### 破坏性变更

- **配置契约升级**：压缩设置统一为 `official_model_settings.gemini`、`claude` 和 `custom_model` 三组嵌套配置。旧版扁平字段不会在运行时自动迁移；升级前请备份并按当前配置结构调整文件。

### 新增功能

- **跨平台宿主接入**：按平台拆分 macOS 与 Windows 的路径、进程、IDE、App 和 CLI 集成；Windows 支持用户级 `CLOUD_CODE_URL` 环境变量的安全接管与恢复。
- **模型与代理能力扩展**：模型目录、模型能力、Token 限额、推理映射和三类模型独立压缩策略统一收敛到清晰的领域模块。
- **完整双语界面**：收口为完整的简体中文和英文翻译，并在构建阶段校验翻译键与静态 DOM 引用。

### 稳定性与安全

- **可靠配置保存**：增加原子替换、权限保护、符号链接拒绝和并发更新保护，避免配置被部分写入。
- **宿主配置可逆**：IDE 设置、App/CLI 环境变量均记录 ownership 和原始值，停用时只恢复仍由 AGY BYOK 管理的内容。
- **代理日志隐私保护**：活动记录不再保存请求正文、响应正文、工具参数或上游敏感错误详情，并过滤官方转发中的本地 `Host` 头。
- **启动错误诊断**：区分 JSON 语法错误与配置结构不匹配，减少升级后的排查成本。

### 构建与发布

- CI 增加 macOS 和 Windows 原生矩阵，覆盖 Rust 检查、测试和桌面端构建。
- 发布流水线覆盖 macOS arm64、macOS x64 和 Windows MSVC 构建，并保留更新清单 URL 校验。

### 验证

- Rust 工作区测试：219 项通过，1 项环境 smoke test 有意忽略。
- macOS 与 Windows GNU 全工作区 Clippy：零警告。
- 前端 TypeScript、Vite 和 i18n 完整性校验通过。
