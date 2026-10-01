use crate::*;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticFile {
    pub name: String,
    pub bytes: Vec<u8>,
    /// Original filesystem time, when it was captured from a v4.1.0 source.
    pub modified_unix: Option<i64>,
}
/// Summary and detail exports captured under the same database observation.
#[derive(Debug)]
pub struct DiagnosticIndex {
    pub summary: IndexSummary,
    pub summary_bytes: Vec<u8>,
    pub summary_modified_unix: Option<i64>,
    pub xml_errors: Result<Option<XmlErrorSource>>,
}
#[derive(Debug)]
pub struct XmlErrorSource {
    pub report: XmlErrorReport,
    pub bytes: Vec<u8>,
    pub modified_unix: Option<i64>,
}
/// Current read-only observations for the original CheckOnly job output.
pub struct IndexDiagnostic {
    pub summary: Option<IndexSummary>,
    pub frontend_ok: bool,
    pub xml_errors_path: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct XmlErrorRow {
    pub path: String,
    /// The new reader's actual content identity, never a fabricated legacy stamp.
    pub stamp: String,
    pub error: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct XmlErrorReport {
    pub generated_at: String,
    pub count: u64,
    pub errors: Vec<XmlErrorRow>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ScanStats {
    pub online_roots_scanned: u64,
    pub nfo_seen: u64,
    pub nfo_reparsed: u64,
    pub technical_specs_found: u64,
    pub web_eligible_specs_found: u64,
    pub episode_specs_excluded_from_web: u64,
    pub xml_read_errors: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SummaryLibrary {
    pub path: String,
    pub kind: String,
    pub online: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IndexSummary {
    pub generated_at: String,
    pub source_task: String,
    pub indexed_titles: u64,
    pub scan_stats: Option<ScanStats>,
    pub libraries: Vec<SummaryLibrary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub library_roots: Vec<String>,
}

pub fn archive_name() -> String {
    // Preserve the existing diagnostic filename, independently of app packages.
    format!(
        "IMDb-Tech-Diagnostics-{}.zip",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    )
}

pub fn write_archive(path: &std::path::Path, files: &[DiagnosticFile]) -> Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| AppError::new("diagnostics-path", "Missing parent folder"))?;
    crate::paths::checked(parent)?;
    let mut names = std::collections::BTreeSet::new();
    for file in files {
        if file.name.is_empty()
            || matches!(file.name.as_str(), "." | "..")
            || file.name.contains(['/', '\\', '\0', ':'])
            || !names.insert(&file.name)
        {
            return Err(AppError::new(
                "diagnostics-entry",
                "Diagnostic filenames must be unique basenames",
            ));
        }
    }
    let mut temp = tempfile::NamedTempFile::new_in(parent)
        .map_err(|e| AppError::new("diagnostics-export", e))?;
    {
        let mut zip = zip::ZipWriter::new(temp.as_file_mut());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o600);
        for file in files {
            let options = file
                .modified_unix
                .and_then(|timestamp| {
                    use chrono::{Datelike, Local, TimeZone, Timelike};
                    let time = Local.timestamp_opt(timestamp, 0).single()?;
                    zip::DateTime::from_date_and_time(
                        time.year().try_into().ok()?,
                        time.month().try_into().ok()?,
                        time.day().try_into().ok()?,
                        time.hour().try_into().ok()?,
                        time.minute().try_into().ok()?,
                        time.second().try_into().ok()?,
                    )
                    .ok()
                })
                .map(|time| options.last_modified_time(time))
                .unwrap_or(options);
            zip.start_file(&file.name, options)
                .map_err(|e| AppError::new("diagnostics-export", e))?;
            // The 4.1.0 exporter retains at most 12 MiB from each input file.
            zip.write_all(&file.bytes[..file.bytes.len().min(12 << 20)])
                .map_err(|e| AppError::new("diagnostics-export", e))?;
        }
        zip.finish()
            .map_err(|e| AppError::new("diagnostics-export", e))?;
    }
    temp.as_file()
        .sync_all()
        .map_err(|e| AppError::new("diagnostics-export", e))?;
    temp.persist_noclobber(path).map_err(|e| {
        AppError::new(
            if e.error.kind() == std::io::ErrorKind::AlreadyExists {
                "diagnostics-exists"
            } else {
                "diagnostics-export"
            },
            &e.error,
        )
        .at(path.display())
    })?;
    Ok(())
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct RootDiagnostic {
    pub root: LibraryRoot,
    pub state: String,
    pub error: Option<AppError>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct NfoDiagnostic {
    pub id: String,
    pub path: String,
    pub title: String,
    pub year: String,
    pub imdb: String,
    pub kind: String,
    pub error: AppError,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct LibraryDiagnostics {
    pub captured_at: String,
    pub database: String,
    pub configuration_revision: u32,
    pub locale: Locale,
    pub roots: Vec<RootDiagnostic>,
    pub total: u32,
    pub errors_total: u32,
    pub errors_truncated: bool,
    pub errors: Vec<NfoDiagnostic>,
    pub recent_tasks: Vec<Task>,
}
