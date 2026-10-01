//! v4.1.0 presentation-only ZIP language packs, pinned by the embedded catalog.
use crate::{hash, services::LOCALES, AppError, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read},
};
use ts_rs::TS;
pub const MAX_BYTES: usize = 8 << 20;
const SECTIONS: [&str; 5] = ["core", "engine", "native", "web", "web-card"];
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Descriptor {
    pub revision: u32,
    pub released_with: String,
    pub asset: String,
    pub sha256: String,
    pub catalog_schema: u32,
    pub message_set_hash: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Catalog {
    pub schema: u32,
    pub product: String,
    pub app_version: String,
    pub languages: BTreeMap<String, Descriptor>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Manifest {
    schema: u32,
    product: String,
    locale: String,
    revision: u32,
    released_with: String,
    catalog_schema: u32,
    message_set_hash: String,
    files: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct LanguageOption {
    pub code: String,
    pub native_name: String,
    pub english_name: String,
    pub flag: String,
    pub built_in: bool,
    pub installed: bool,
    pub downloadable: bool,
    pub state: String,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct LanguageSnapshot {
    pub locale: crate::Locale,
    pub options: Vec<LanguageOption>,
    pub web_messages: BTreeMap<String, String>,
    pub native_messages: BTreeMap<String, String>,
}
pub type Sections = BTreeMap<String, BTreeMap<String, String>>;
fn invalid(message: impl ToString) -> AppError {
    AppError::new("language-pack-invalid", message)
}
fn digest(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit())
}
impl Catalog {
    /// Pinned published descriptors, solely for reusing already installed v4 packs.
    /// Downloads always use `embedded`; this does not discover older Releases.
    pub fn previous_installation() -> Result<Self> {
        let mut catalog: Self =
            serde_json::from_str(include_str!("../../../../windows/language_catalog.json"))?;
        catalog.app_version = format!("v{}", env!("CARGO_PKG_VERSION"));
        catalog.validate()?;
        Ok(catalog)
    }
    pub fn embedded() -> Result<Self> {
        let catalog: Self = serde_json::from_str(include_str!("../assets/language_catalog.json"))?;
        catalog.validate()?;
        Ok(catalog)
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema != 1
            || self.product != "tcm"
            || self.app_version != format!("v{}", env!("CARGO_PKG_VERSION"))
            || self.languages.len() != 5
        {
            return Err(invalid("语言包目录不属于当前应用版本"));
        }
        for locale in LOCALES.iter().filter(|l| !l.built_in) {
            let d = self
                .languages
                .get(locale.code)
                .ok_or_else(|| invalid("当前版本没有为该语言指定语言包"))?;
            if d.revision == 0
                || d.catalog_schema != 1
                || !digest(&d.sha256)
                || !digest(&d.message_set_hash)
                || d.asset != format!("TCM-Language-{}-r{}.zip", locale.code, d.revision)
                || !d.released_with.starts_with('v')
                || crate::manual_update::stable_version(&d.released_with).is_err()
            {
                return Err(invalid("语言包目录记录无效"));
            }
        }
        Ok(())
    }
    pub fn descriptor(&self, locale: &str) -> Result<&Descriptor> {
        self.validate()?;
        self.languages
            .get(locale)
            .ok_or_else(|| invalid("该语言不需要下载"))
    }
    pub fn url(&self, locale: &str) -> Result<String> {
        let d = self.descriptor(locale)?;
        Ok(format!(
            "https://github.com/Eric-Hou1997/Tech-Card-Manager/releases/download/{}/{}",
            d.released_with, d.asset
        ))
    }
    pub fn decode(&self, locale: &str, bytes: &[u8]) -> Result<Sections> {
        let d = self.descriptor(locale)?;
        if bytes.len() > MAX_BYTES {
            return Err(invalid("语言包过大，已拒绝安装"));
        }
        if !hash(bytes).eq_ignore_ascii_case(&d.sha256) {
            return Err(invalid("语言包摘要验证失败"));
        }
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| invalid(format!("语言包 ZIP 无效：{e}")))?;
        if zip.len() != 6 {
            return Err(invalid("语言包文件集合无效"));
        }
        let allowed: BTreeSet<_> = SECTIONS
            .iter()
            .map(|s| format!("{s}.json"))
            .chain(std::iter::once("manifest.json".into()))
            .collect();
        let mut files = BTreeMap::new();
        let mut total = 0;
        for index in 0..zip.len() {
            let file = zip.by_index(index).map_err(invalid)?;
            let name = file.name().to_owned();
            if !allowed.contains(&name)
                || files.contains_key(&name)
                || file.is_dir()
                || file
                    .unix_mode()
                    .is_some_and(|m| m & 0o170000 != 0 && m & 0o170000 != 0o100000)
                || file.size() > 4 << 20
            {
                return Err(invalid("语言包包含不允许的文件或内容过大"));
            }
            let mut bytes = Vec::new();
            file.take((4 << 20) + 1)
                .read_to_end(&mut bytes)
                .map_err(invalid)?;
            total += bytes.len();
            if bytes.len() > 4 << 20 || total > MAX_BYTES {
                return Err(invalid("语言包内容过大"));
            }
            files.insert(name, bytes);
        }
        self.decode_installed_files(locale, &files)
    }
    /// Validate the v4.1.0 installed-file contract. This verifies the manifest
    /// and its section hashes; extracted files cannot prove a ZIP archive hash.
    pub fn decode_installed_files(
        &self,
        locale: &str,
        files: &BTreeMap<String, Vec<u8>>,
    ) -> Result<Sections> {
        let d = self.descriptor(locale)?;
        let expected: BTreeSet<_> = SECTIONS
            .iter()
            .map(|s| format!("{s}.json"))
            .chain(std::iter::once("manifest.json".into()))
            .collect();
        if files.keys().cloned().collect::<BTreeSet<_>>() != expected {
            return Err(invalid("语言包文件集合无效"));
        }
        if files.values().any(|b| b.len() > 4 << 20)
            || files.values().map(Vec::len).sum::<usize>() > MAX_BYTES
        {
            return Err(invalid("语言包内容过大"));
        }
        let manifest: Manifest =
            serde_json::from_slice(&files["manifest.json"]).map_err(invalid)?;
        if manifest.schema != 1
            || manifest.product != self.product
            || manifest.locale != locale
            || manifest.revision != d.revision
            || manifest.released_with != d.released_with
            || manifest.catalog_schema != 1
            || manifest.message_set_hash != d.message_set_hash
            || manifest.files.len() != 5
        {
            return Err(invalid("语言包清单与当前应用目录不匹配"));
        }
        let mut sections = Sections::new();
        for section in SECTIONS {
            let name = format!("{section}.json");
            let bytes = &files[&name];
            if manifest
                .files
                .get(&name)
                .is_none_or(|sha| !hash(bytes).eq_ignore_ascii_case(sha))
            {
                return Err(invalid("语言包文件摘要验证失败"));
            }
            let messages: BTreeMap<String, String> =
                serde_json::from_slice(bytes).map_err(invalid)?;
            if messages.iter().any(|(id, text)| {
                !id.starts_with("legacy.")
                    || id.len() != 23
                    || !id[7..].bytes().all(|c| c.is_ascii_hexdigit())
                    || text.trim().is_empty()
                    || text.contains('\0')
            }) {
                return Err(invalid("语言包文本目录无效"));
            }
            sections.insert(section.into(), messages);
        }
        Ok(sections)
    }
}
pub fn options() -> Vec<LanguageOption> {
    let english = [
        "Simplified Chinese",
        "Traditional Chinese",
        "English (United States)",
        "French",
        "Russian",
        "Japanese",
        "Spanish",
        "Thai",
    ];
    let flags = ["cn", "cn", "us", "fr", "ru", "jp", "es", "th"];
    LOCALES
        .iter()
        .enumerate()
        .map(|(i, l)| LanguageOption {
            code: l.code.into(),
            native_name: l.native_name.into(),
            english_name: english[i].into(),
            flag: flags[i].into(),
            built_in: l.built_in,
            installed: l.built_in,
            downloadable: !l.built_in,
            state: if l.built_in {
                "built-in"
            } else {
                "not-installed"
            }
            .into(),
            error: None,
        })
        .collect()
}
