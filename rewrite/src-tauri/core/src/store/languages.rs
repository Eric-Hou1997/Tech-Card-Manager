use super::*;
use crate::languages::{Catalog, Sections, MAX_BYTES};
use base64::Engine;
impl Store {
    pub fn legacy_language_pack(
        &self,
        catalog: &Catalog,
        locale: &str,
    ) -> Result<Option<Sections>> {
        match self.current_language_pack(catalog, locale)? {
            Some(sections) => Ok(Some(sections)),
            None if catalog.descriptor(locale)?.revision > 1 => {
                self.current_language_pack(&Catalog::previous_installation()?, locale)
            }
            None => Ok(None),
        }
    }
    pub fn current_language_pack(
        &self,
        catalog: &Catalog,
        locale: &str,
    ) -> Result<Option<Sections>> {
        let d = catalog.descriptor(locale)?;
        let key = format!("language-zip:{}", d.sha256);
        let row:Option<Option<String>>=self.db()?.query_row("SELECT CASE WHEN length(CAST(body AS BLOB))<=?2 THEN body ELSE NULL END FROM preferences WHERE key=?1",params![key,MAX_BYTES*2],|r|r.get(0)).optional()?;
        let body = match row {
            None => return self.archived_language_pack(catalog, locale),
            Some(Some(body)) => body,
            Some(None) => return Err(AppError::new("language-size", "语言包缓存过大")),
        };
        let encoded: String = serde_json::from_str(&body)?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|e| AppError::new("language-cache-invalid", e))?;
        catalog.decode(locale, &bytes).map(Some)
    }
    fn archived_language_pack(&self, catalog: &Catalog, locale: &str) -> Result<Option<Sections>> {
        let descriptor = catalog.descriptor(locale)?;
        let prefix = format!("language-packs/{locale}/r{}/", descriptor.revision);
        let data_prefix = format!("data/{prefix}");
        let db = self.db()?;
        // Select one revision directory in one import. A damaged newer import
        // must not silently borrow missing sections from a previous snapshot.
        let latest:Option<(String,String)>=db.query_row(
            "SELECT import_id,path FROM legacy_artifacts WHERE category='language-pack' AND (substr(path,1,length(?1))=?1 OR substr(path,1,length(?2))=?2) ORDER BY rowid DESC LIMIT 1",
            params![prefix,data_prefix],|row|Ok((row.get(0)?,row.get(1)?)),
        ).optional()?;
        let Some((import_id, path)) = latest else {
            return Ok(None);
        };
        let other_prefix = if path.starts_with(&data_prefix) {
            &prefix
        } else {
            &data_prefix
        };
        if db.query_row("SELECT EXISTS(SELECT 1 FROM legacy_artifacts WHERE import_id=?1 AND category='language-pack' AND substr(path,1,length(?2))=?2)",params![import_id,other_prefix],|row|row.get::<_,bool>(0))? {
            return Err(AppError::new("language-archive-ambiguous","同一次导入包含多个旧语言包目录，无法确认使用哪一份"));
        }
        let prefix = if path.starts_with(&data_prefix) {
            data_prefix
        } else {
            prefix
        };
        let mut files = std::collections::BTreeMap::new();
        let mut total = 0;
        for name in [
            "manifest.json",
            "core.json",
            "engine.json",
            "native.json",
            "web.json",
            "web-card.json",
        ] {
            let path = format!("{prefix}{name}");
            let row:Option<(String,Option<Vec<u8>>)>=db.query_row(
                "SELECT sha256,CASE WHEN length(body)<=?3 THEN body ELSE NULL END FROM legacy_artifacts WHERE import_id=?1 AND path=?2 AND category='language-pack'",
                params![import_id,path,4<<20],|row|Ok((row.get(0)?,row.get(1)?)),
            ).optional()?;
            let Some((expected, Some(bytes))) = row else {
                return Err(AppError::new(
                    "language-archive-incomplete",
                    "旧语言包缺少完整文件或内容过大",
                )
                .at(path));
            };
            if hash(&bytes) != expected {
                return Err(AppError::new(
                    "language-archive-integrity",
                    "旧语言包文件与导入时的摘要不一致",
                )
                .at(path));
            }
            total += bytes.len();
            if total > MAX_BYTES {
                return Err(AppError::new("language-size", "旧语言包内容过大"));
            }
            files.insert(name.into(), bytes);
        }
        catalog.decode_installed_files(locale, &files).map(Some)
    }
    pub fn install_legacy_language_pack(
        &self,
        catalog: &Catalog,
        locale: &str,
        bytes: &[u8],
    ) -> Result<Sections> {
        let sections = catalog.decode(locale, bytes)?;
        let d = catalog.descriptor(locale)?;
        let body = serde_json::to_string(&base64::engine::general_purpose::STANDARD.encode(bytes))?;
        let mut db = self.db()?;
        self.writable()?;
        let tx = db.transaction()?;
        tx.execute("INSERT INTO preferences(key,body) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET body=excluded.body",params![format!("language-zip:{}",d.sha256),body])?;
        tx.execute("INSERT INTO preferences(key,body) VALUES(?1,'true') ON CONFLICT(key) DO UPDATE SET body=excluded.body",[format!("language-install-history:{locale}")])?;
        tx.commit()?;
        Ok(sections)
    }
    pub fn language_restore_candidates(&self, catalog: &Catalog) -> Result<Vec<String>> {
        catalog.validate()?;
        let configured = self.configuration()?.locale;
        let mut targets = Vec::new();
        for option in crate::services::LOCALES.iter().filter(|l| !l.built_in) {
            if self
                .current_language_pack(catalog, option.code)
                .is_ok_and(|p| p.is_some())
            {
                continue;
            }
            let mut present = configured.as_str() == option.code;
            if !present {
                let db = self.db()?;
                present = db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM preferences WHERE key=?1 AND body='true')",
                    [format!("language-install-history:{}", option.code)],
                    |r| r.get(0),
                )?;
                if !present {
                    let prefix = format!("language-packs/{}/", option.code);
                    let data_prefix = format!("data/{prefix}");
                    let mut statement=db.prepare("SELECT path FROM legacy_artifacts WHERE category='language-pack' AND (substr(path,1,length(?1))=?1 OR substr(path,1,length(?2))=?2)")?;
                    for path in statement
                        .query_map(params![prefix, data_prefix], |r| r.get::<_, String>(0))?
                    {
                        let path = path?;
                        let relative = path
                            .strip_prefix(&prefix)
                            .or_else(|| path.strip_prefix(&data_prefix))
                            .unwrap_or("");
                        let revision = relative.split('/').next().unwrap_or("");
                        let number = revision.strip_prefix('r').unwrap_or("");
                        if relative.contains('/')
                            && !number.is_empty()
                            && !number.starts_with('0')
                            && number.bytes().all(|c| c.is_ascii_digit())
                        {
                            present = true;
                            break;
                        }
                    }
                }
            }
            if present {
                targets.push(option.code.into());
            }
        }
        Ok(targets)
    }
    pub fn select_ui_language(&self, catalog: &Catalog, locale: Locale) -> Result<Configuration> {
        if !crate::services::LOCALES
            .iter()
            .find(|l| l.code == locale.as_str())
            .is_some_and(|l| l.built_in)
            && self
                .legacy_language_pack(catalog, locale.as_str())?
                .is_none()
        {
            return Err(AppError::new("language-pack-required", "请先下载该语言包"));
        }
        let mut db = self.db()?;
        self.writable()?;
        let tx = db.transaction()?;
        let body: Option<String> = tx
            .query_row("SELECT body FROM configuration WHERE id=1", [], |r| {
                r.get(0)
            })
            .optional()?;
        let mut config: Configuration = body
            .map(|value| serde_json::from_str(&value))
            .transpose()?
            .unwrap_or_default();
        if config.locale != locale {
            config.locale = locale;
            config.revision = config
                .revision
                .checked_add(1)
                .ok_or_else(|| AppError::new("configuration-revision", "配置修订号溢出"))?;
            tx.execute(
                "INSERT INTO configuration(id,body) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET body=excluded.body",
                [serde_json::to_string(&config)?],
            )?;
        }
        tx.commit()?;
        Ok(config)
    }
}
