use agy_byok::storage::default_config_path;
use std::fs;
use std::path::PathBuf;

const NATIVE_LOCALE_FILE: &str = "native-locale";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeLocale {
    ZhCn,
    EnUs,
}

impl NativeLocale {
    pub fn detect() -> Self {
        sys_locale::get_locale()
            .as_deref()
            .map(Self::from_tag)
            .unwrap_or(Self::EnUs)
    }

    pub fn preferred() -> Self {
        native_locale_path()
            .ok()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|tag| Self::from_saved_tag(tag.trim()))
            .unwrap_or_else(Self::detect)
    }

    pub fn from_tag(tag: &str) -> Self {
        if tag.trim().to_ascii_lowercase().starts_with("zh") {
            Self::ZhCn
        } else {
            Self::EnUs
        }
    }

    pub fn persist(self) -> Result<(), String> {
        let path = native_locale_path()?;
        if path.is_symlink() {
            return Err("native locale preference must not be a symbolic link".to_string());
        }
        let parent = path
            .parent()
            .ok_or_else(|| "native locale preference has no parent directory".to_string())?;
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "failed to create native locale preference directory {}: {error}",
                parent.display()
            )
        })?;
        fs::write(&path, self.tag()).map_err(|error| {
            format!(
                "failed to write native locale preference {}: {error}",
                path.display()
            )
        })
    }

    const fn tag(self) -> &'static str {
        match self {
            Self::ZhCn => "zh-CN",
            Self::EnUs => "en-US",
        }
    }

    fn from_saved_tag(tag: &str) -> Option<Self> {
        match tag {
            "zh-CN" => Some(Self::ZhCn),
            "en-US" => Some(Self::EnUs),
            _ => None,
        }
    }

    pub const fn tray_show(self) -> &'static str {
        match self {
            Self::ZhCn => "显示 AGY BYOK",
            Self::EnUs => "Show AGY BYOK",
        }
    }

    pub const fn tray_quit(self) -> &'static str {
        match self {
            Self::ZhCn => "退出",
            Self::EnUs => "Quit",
        }
    }

    pub const fn startup_error_title(self) -> &'static str {
        match self {
            Self::ZhCn => "AGY BYOK 启动失败",
            Self::EnUs => "AGY BYOK failed to start",
        }
    }
}

fn native_locale_path() -> Result<PathBuf, String> {
    let config_path = default_config_path()?;
    let parent = config_path
        .parent()
        .ok_or_else(|| "configuration path has no parent directory".to_string())?;
    Ok(parent.join(NATIVE_LOCALE_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_tags_only_select_supported_native_languages() {
        assert_eq!(NativeLocale::from_tag("zh-CN"), NativeLocale::ZhCn);
        assert_eq!(NativeLocale::from_tag("zh-Hant-TW"), NativeLocale::ZhCn);
        assert_eq!(NativeLocale::from_tag("en-US"), NativeLocale::EnUs);
        assert_eq!(NativeLocale::from_tag("de-DE"), NativeLocale::EnUs);
        assert_eq!(
            NativeLocale::from_saved_tag("zh-CN"),
            Some(NativeLocale::ZhCn)
        );
        assert_eq!(
            NativeLocale::from_saved_tag("en-US"),
            Some(NativeLocale::EnUs)
        );
        assert_eq!(NativeLocale::from_saved_tag("zh-TW"), None);
    }
}
