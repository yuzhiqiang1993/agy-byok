# 更新日志

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
