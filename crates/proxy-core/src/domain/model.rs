//! 模型领域的稳定入口。
//!
//! 各子模块按检查点、推理、Token 与模型定义拆分；这里保留原有的
//! `domain::model::*` 访问路径，避免领域调用方感知内部文件布局。

mod checkpoint;
mod definition;
mod reasoning;
mod token_limits;
mod tokenizer;

pub use checkpoint::{CustomModelCheckpointRetryConfig, ModelCompressionPolicy};
pub use definition::{
    stable_hash, ModelCapabilities, ModelModality, ModelRole, UpstreamModel, VirtualModel,
};
pub use reasoning::{ReasoningCapability, ReasoningLevel, ReasoningMapping};
pub use token_limits::{ModelTokenLimits, TokenLimitSource};
pub use tokenizer::{TiktokenEncoding, TokenizerConfig};
