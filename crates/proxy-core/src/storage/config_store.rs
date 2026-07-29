use crate::domain::{Provider, UpstreamModel, VirtualModel};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub providers: Vec<Provider>,
    pub upstream_models: Vec<UpstreamModel>,
    pub virtual_models: Vec<VirtualModel>,
}

#[derive(Clone)]
pub struct ConfigStore {
    config: Arc<RwLock<AppConfig>>,
    file_path: Option<PathBuf>,
}

impl ConfigStore {
    pub fn in_memory(initial_config: AppConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(initial_config)),
            file_path: None,
        }
    }

    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self, String> {
        let path_buf = path.as_ref().to_path_buf();
        let config = if path_buf.exists() {
            let content = fs::read_to_string(&path_buf)
                .map_err(|e| format!("Failed to read config: {}", e))?;
            serde_json::from_str(&content).map_err(|e| format!("Failed to parse config: {}", e))?
        } else {
            AppConfig::default()
        };

        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            file_path: Some(path_buf),
        })
    }

    pub fn get_config(&self) -> AppConfig {
        self.config.read().unwrap().clone()
    }

    pub fn update_config(&self, new_config: AppConfig) -> Result<(), String> {
        if let Some(ref path) = self.file_path {
            let json_content = serde_json::to_string_pretty(&new_config)
                .map_err(|e| format!("Failed to serialize config: {}", e))?;

            let tmp_path = path.with_extension("tmp");
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create config dir: {}", e))?;
            }

            fs::write(&tmp_path, json_content)
                .map_err(|e| format!("Failed to write tmp config: {}", e))?;
            fs::rename(&tmp_path, path)
                .map_err(|e| format!("Failed to replace config file: {}", e))?;
        }

        let mut guard = self.config.write().unwrap();
        *guard = new_config;
        Ok(())
    }
}
