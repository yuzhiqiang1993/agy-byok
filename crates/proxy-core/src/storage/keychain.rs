use crate::domain::{ErrorCategory, ProxyError};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;

const SERVICE_NAME: &str = "com.yuzhiqiang.agy-byok";

#[async_trait]
pub trait KeyStore: Send + Sync {
    async fn set_secret(&self, key_ref: &str, secret: &str) -> Result<(), ProxyError>;
    async fn get_secret(&self, key_ref: &str) -> Result<String, ProxyError>;
    async fn delete_secret(&self, key_ref: &str) -> Result<(), ProxyError>;
}

/// 基于 macOS Keychain 的系统秘钥存储实现
#[derive(Default)]
pub struct KeychainStore;

impl KeychainStore {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl KeyStore for KeychainStore {
    async fn set_secret(&self, key_ref: &str, secret: &str) -> Result<(), ProxyError> {
        let entry = keyring::Entry::new(SERVICE_NAME, key_ref)
            .map_err(|e| ProxyError::new(ErrorCategory::Internal, e.to_string(), 500))?;
        entry
            .set_password(secret)
            .map_err(|e| ProxyError::new(ErrorCategory::Internal, e.to_string(), 500))?;
        Ok(())
    }

    async fn get_secret(&self, key_ref: &str) -> Result<String, ProxyError> {
        let entry = keyring::Entry::new(SERVICE_NAME, key_ref)
            .map_err(|e| ProxyError::new(ErrorCategory::Authentication, e.to_string(), 401))?;
        entry.get_password().map_err(|e| {
            ProxyError::new(
                ErrorCategory::Authentication,
                format!("Failed to retrieve secret for key_ref {}: {}", key_ref, e),
                401,
            )
        })
    }

    async fn delete_secret(&self, key_ref: &str) -> Result<(), ProxyError> {
        let entry = keyring::Entry::new(SERVICE_NAME, key_ref)
            .map_err(|e| ProxyError::new(ErrorCategory::Internal, e.to_string(), 500))?;
        let _ = entry.delete_password();
        Ok(())
    }
}

/// 内存秘钥存储实现（用于单元测试及 Mock 隔离）
#[derive(Default)]
pub struct MemoryKeyStore {
    secrets: Mutex<HashMap<String, String>>,
}

impl MemoryKeyStore {
    pub fn new() -> Self {
        Self {
            secrets: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl KeyStore for MemoryKeyStore {
    async fn set_secret(&self, key_ref: &str, secret: &str) -> Result<(), ProxyError> {
        let mut guard = self.secrets.lock().unwrap();
        guard.insert(key_ref.to_string(), secret.to_string());
        Ok(())
    }

    async fn get_secret(&self, key_ref: &str) -> Result<String, ProxyError> {
        let guard = self.secrets.lock().unwrap();
        guard.get(key_ref).cloned().ok_or_else(|| {
            ProxyError::new(
                ErrorCategory::Authentication,
                format!("Secret not found in memory store: {}", key_ref),
                401,
            )
        })
    }

    async fn delete_secret(&self, key_ref: &str) -> Result<(), ProxyError> {
        let mut guard = self.secrets.lock().unwrap();
        guard.remove(key_ref);
        Ok(())
    }
}
