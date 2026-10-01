//! Read-only presentation of imported task summaries and original log text.
//! The allowlist deliberately excludes configuration, AI/provider caches and backups.
use crate::*;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct HistoryArchive {
    pub import_id: String,
    pub path: String,
    pub sha256: String,
    #[ts(type = "number")]
    pub bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct HistoryArchives {
    pub total: u32,
    pub items: Vec<HistoryArchive>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct HistoryPage {
    pub archive: HistoryArchive,
    pub offset: u32,
    pub next_offset: Option<u32>,
    pub unit: String,
    pub text: String,
    pub tasks: Vec<serde_json::Value>,
}
pub type DiagnosticLog = (HistoryArchive, Vec<u8>, Option<i64>);
pub(crate) const ALLOWED: &str = "((category='history' AND path LIKE '%.log') OR (category='legacy-state' AND path IN ('task-history.json','data/task-history.json')))";
pub(crate) fn page(archive: HistoryArchive, bytes: Vec<u8>, offset: u32) -> Result<HistoryPage> {
    if hash(&bytes) != archive.sha256 {
        return Err(AppError::new(
            "history-integrity",
            "Historical file failed its original checksum",
        )
        .at(&archive.path));
    }
    let text = String::from_utf8(bytes).map_err(|e| {
        AppError::new(
            "history-encoding",
            format!("Historical bytes are preserved but cannot be displayed as UTF-8: {e}"),
        )
        .at(&archive.path)
    })?;
    if archive.path.ends_with("task-history.json") {
        if let Ok(serde_json::Value::Array(tasks)) =
            serde_json::from_str(text.trim_start_matches('\u{feff}'))
        {
            if tasks
                .iter()
                .all(|task| task.is_object() && task.to_string().len() <= 16 * 1024)
            {
                let start = offset as usize;
                if start > tasks.len() {
                    return Err(AppError::new(
                        "history-offset",
                        "Historical record offset is out of range",
                    ));
                }
                let end = (start + 50).min(tasks.len());
                return Ok(HistoryPage {
                    archive,
                    offset,
                    next_offset: (end < tasks.len()).then_some(end as u32),
                    unit: "records".into(),
                    text: String::new(),
                    tasks: tasks[start..end].to_vec(),
                });
            }
        }
        // Malformed or unknown old records remain inspectable as original text.
    }
    let start = offset as usize;
    if start > text.len() || !text.is_char_boundary(start) {
        return Err(AppError::new(
            "history-offset",
            "Historical text offset must be a UTF-8 boundary",
        ));
    }
    let mut end = (start + 64 * 1024).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Ok(HistoryPage {
        archive,
        offset,
        next_offset: (end < text.len()).then_some(end as u32),
        unit: "bytes".into(),
        text: text[start..end].into(),
        tasks: vec![],
    })
}
