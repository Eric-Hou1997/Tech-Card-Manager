//! Infrastructure boundaries. Secrets never enter serializable UI/configuration types.
use crate::Result;
pub trait CredentialStore: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<String>>;
    fn put(&self, account: &str, secret: &str) -> Result<()>;
    fn delete(&self, account: &str) -> Result<()>;
}
pub struct LocaleDescriptor {
    pub code: &'static str,
    pub native_name: &'static str,
    pub built_in: bool,
}
pub const LOCALES: [LocaleDescriptor; 8] = [
    LocaleDescriptor {
        code: "zh-CN",
        native_name: "简体中文",
        built_in: true,
    },
    LocaleDescriptor {
        code: "zh-Hant",
        native_name: "繁體中文",
        built_in: true,
    },
    LocaleDescriptor {
        code: "en-US",
        native_name: "English",
        built_in: true,
    },
    LocaleDescriptor {
        code: "fr-FR",
        native_name: "Français",
        built_in: false,
    },
    LocaleDescriptor {
        code: "ru-RU",
        native_name: "Русский",
        built_in: false,
    },
    LocaleDescriptor {
        code: "ja-JP",
        native_name: "日本語",
        built_in: false,
    },
    LocaleDescriptor {
        code: "es-ES",
        native_name: "Español",
        built_in: false,
    },
    LocaleDescriptor {
        code: "th-TH",
        native_name: "ไทย",
        built_in: false,
    },
];
