//! Original backend phrases captured once per task. Callers pass presentation
//! fragments only; paths, media names and protocol evidence are never translated.
use crate::{
    languages::{Catalog, Sections},
    store::Store,
    Locale, Result,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct TaskText {
    locale: String,
    pairs: Vec<(String, String, String)>,
}

fn message_id(value: &str) -> String {
    let mut hash = 14695981039346656037u64;
    for byte in value.trim().bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(1099511628211);
    }
    format!("legacy.{hash:016x}")
}
fn traditional(value: &str, pairs: &[(String, String)]) -> String {
    let mut rest = value;
    let mut result = String::new();
    while !rest.is_empty() {
        if let Some((source, target)) = pairs.iter().find(|(source, _)| rest.starts_with(source)) {
            result.push_str(target);
            rest = &rest[source.len()..];
        } else {
            let ch = rest.chars().next().unwrap();
            result.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    result
}
impl TaskText {
    pub(crate) fn capture(store: &Store, locale: &Locale) -> Result<Self> {
        let sections = if matches!(
            locale,
            Locale::Simplified | Locale::Traditional | Locale::English
        ) {
            None
        } else {
            store.legacy_language_pack(&Catalog::embedded()?, locale.as_str())?
        };
        Self::from_sections(locale, sections.as_ref())
    }
    fn from_sections(locale: &Locale, sections: Option<&Sections>) -> Result<Self> {
        if *locale == Locale::Simplified {
            return Ok(Self {
                locale: locale.as_str().into(),
                pairs: vec![],
            });
        }
        let mut phrases: Vec<(String, String)> = serde_json::from_str(include_str!(
            "../../../src/assets/baseline-backend-phrases.json"
        ))?;
        phrases.sort_by_key(|(zh, _)| std::cmp::Reverse(zh.len()));
        let baseline: serde_json::Value =
            serde_json::from_str(include_str!("../../../src/assets/baseline-languages.json"))?;
        let trad: Vec<(String, String)> = serde_json::from_value(baseline["traditional"].clone())?;
        let pairs = phrases
            .into_iter()
            .map(|(zh, en)| {
                let translated = if *locale == Locale::Traditional {
                    traditional(&zh, &trad)
                } else {
                    let id = message_id(&en);
                    sections
                        .and_then(|sections| {
                            sections
                                .get("core")
                                .and_then(|s| s.get(&id))
                                .or_else(|| sections.get("engine").and_then(|s| s.get(&id)))
                        })
                        .cloned()
                        .unwrap_or_else(|| en.clone())
                };
                (zh, en, translated)
            })
            .collect();
        Ok(Self {
            locale: locale.as_str().into(),
            pairs,
        })
    }
    pub(crate) fn text(&self, value: &str) -> String {
        if self.locale.is_empty() || self.locale == "zh-CN" {
            return value.into();
        }
        let mut result = value.to_string();
        for (zh, en, translated) in &self.pairs {
            result = result.replace(zh, translated);
            if self.locale != "en-US" {
                result = result.replace(en, translated);
            }
        }
        result
    }
}

impl Store {
    /// Present an existing backend message in the currently selected language.
    /// This changes presentation text only; error codes, paths and operation
    /// identities remain separate structured fields.
    pub fn localize_backend_text(&self, value: &str) -> Result<String> {
        let configuration = self.configuration()?;
        Ok(TaskText::capture(self, &configuration.locale)?.text(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_phrase_ids_builtin_text_and_external_precedence_are_preserved() {
        assert_eq!(
            TaskText::from_sections(&Locale::Simplified, None)
                .unwrap()
                .text("运行中"),
            "运行中"
        );
        assert_eq!(
            TaskText::from_sections(&Locale::English, None)
                .unwrap()
                .text("运行中"),
            "Running"
        );
        assert_eq!(
            TaskText::from_sections(&Locale::English, None)
                .unwrap()
                .text("任务已取消"),
            "The task was cancelled"
        );
        assert_eq!(
            TaskText::from_sections(&Locale::English, None)
                .unwrap()
                .text("扫描停止等待超过 8 秒，请稍后重试退出"),
            "Waiting for the scan to stop exceeded 8 seconds. Retry exit later"
        );
        assert_eq!(
            TaskText::from_sections(&Locale::English, None)
                .unwrap()
                .text("卡片服务发布线程停止等待超过 8 秒，请稍后重试退出"),
            "Waiting for the card publication thread to stop exceeded 8 seconds. Retry exit later"
        );
        assert_eq!(
            TaskText::from_sections(&Locale::English, None)
                .unwrap()
                .text("卡片服务续租线程停止等待超过 8 秒，请稍后重试退出"),
            "Waiting for the card lease-renewal thread to stop exceeded 8 seconds. Retry exit later"
        );
        assert_eq!(
            TaskText::from_sections(&Locale::Traditional, None)
                .unwrap()
                .text("状态"),
            "狀態"
        );
        assert_eq!(message_id(" Running "), message_id("Running"));
        for locale in [
            Locale::French,
            Locale::Russian,
            Locale::Japanese,
            Locale::Spanish,
            Locale::Thai,
        ] {
            let id = message_id("Running");
            let sections = Sections::from([
                (
                    "core".into(),
                    [(id.clone(), "core translation".into())].into(),
                ),
                ("engine".into(), [(id, "engine translation".into())].into()),
            ]);
            let text = TaskText::from_sections(&locale, Some(&sections)).unwrap();
            assert_eq!(text.text("运行中"), "core translation");
            assert_eq!(text.text("Running"), "core translation");
            assert_eq!(
                TaskText::from_sections(&locale, None)
                    .unwrap()
                    .text("运行中"),
                "Running"
            );
            let restored: TaskText =
                serde_json::from_str(&serde_json::to_string(&text).unwrap()).unwrap();
            assert_eq!(restored.text("运行中"), "core translation");
        }
    }

    #[test]
    fn current_store_language_localizes_native_error_detail_without_changing_source_state() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
        assert_eq!(
            store.localize_backend_text("服务启动失败").unwrap(),
            "服务启动失败"
        );
        store
            .select_ui_language(&Catalog::embedded().unwrap(), Locale::English)
            .unwrap();
        assert_eq!(
            store.localize_backend_text("服务启动失败").unwrap(),
            "Service startup failed"
        );
        assert_eq!(store.configuration().unwrap().locale, Locale::English);
    }
}
