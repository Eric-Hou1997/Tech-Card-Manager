//! Legacy import is a read-only snapshot followed by one SQLite transaction.
//! No legacy task is resumed and no legacy updater snapshot authorizes an update.
use crate::{hash, paths, AppError, Configuration, LibraryRoot, Locale, Result, Space};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use ts_rs::TS;
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct LegacyFile {
    pub relative: String,
    pub hash: String,
    pub bytes: u64,
    pub category: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_unix: Option<i64>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct LegacyRoot {
    pub path: String,
    pub space: Option<Space>,
    pub enabled: bool,
    pub state: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct MigrationPlan {
    pub id: String,
    pub source: String,
    pub source_kind: String,
    pub fingerprint: String,
    pub configuration_revision: u32,
    #[serde(default)]
    pub incremental: Option<crate::incremental::IncrementalSettings>,
    pub files: Vec<LegacyFile>,
    pub roots: Vec<LegacyRoot>,
    pub locale: Option<String>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct MigrationReceipt {
    pub id: String,
    pub source: String,
    pub fingerprint: String,
    pub imported_files: u32,
    pub imported_bytes: String,
    pub pending_roots: Vec<LegacyRoot>,
    pub phase: String,
    pub configuration: Configuration,
}
const FILE_LIMIT: u64 = 64 * 1024 * 1024;
fn error(code: &str, message: impl ToString, path: &Path) -> AppError {
    AppError::new(code, message).at(path.display())
}
fn category(relative: &str) -> Option<&'static str> {
    let relative = relative.strip_prefix("data/").unwrap_or(relative);
    let first = relative.split('/').next()?;
    let name = relative.rsplit('/').next()?;
    if name.ends_with(".pem")
        || name.ends_with(".key")
        || matches!(
            first,
            "runtime" | "chrome-profile" | "updates" | "browser-profile" | "node_modules"
        )
        || name.ends_with(".lock")
        || name.ends_with(".pid")
    {
        return None;
    }
    match first {
        "logs" => Some("history"),
        "cache" => Some("imdb-cache"),
        "ai-cache" => Some("ai-cache"),
        "ownership" => Some("ownership"),
        "undo" | "backup" | "backups" => Some("backup"),
        "language-packs" => Some("language-pack"),
        _ if matches!(name, "settings.json" | "config.json" | "ui-layout.json") => {
            Some("configuration")
        }
        _ if name.ends_with(".json") => Some("legacy-state"),
        _ if name.ends_with(".log") => Some("history"),
        _ => None,
    }
}
fn walk(
    root: &Path,
    dir: &Path,
    out: &mut Vec<LegacyFile>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    for entry in fs::read_dir(dir).map_err(|e| error("migration-read", e, dir))? {
        let path = entry.map_err(|e| error("migration-read", e, dir))?.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|e| error("migration-path", e, &path))?
            .to_str()
            .ok_or_else(|| error("migration-encoding", "Path must be Unicode", &path))?
            .replace('\\', "/");
        let meta = fs::symlink_metadata(&path).map_err(|e| error("migration-read", e, &path))?;
        if meta.is_dir() {
            if matches!(
                relative.as_str(),
                "data"
                    | "logs"
                    | "cache"
                    | "ai-cache"
                    | "ownership"
                    | "undo"
                    | "backup"
                    | "backups"
                    | "language-packs"
            ) || relative.contains('/') && category(&relative).is_some()
            {
                paths::checked(&path)?;
                walk(root, &path, out, warnings)?;
            }
            continue;
        }
        let Some(category) = category(&relative) else {
            continue;
        };
        paths::within(root, &path)?;
        if !meta.is_file() || meta.len() > FILE_LIMIT {
            return Err(error(
                "migration-file-limit",
                "Expected a regular file no larger than 64 MiB",
                &path,
            ));
        }
        let bytes = fs::read(&path).map_err(|e| error("migration-read", e, &path))?;
        if relative.ends_with(".json") {
            match serde_json::from_slice::<Value>(
                bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes),
            ) {
                Ok(value) => {
                    if secret_field(&value) {
                        return Err(error("migration-credential-boundary","Credential-bearing JSON needs native credential migration before import",&path));
                    }
                }
                Err(_) => {
                    if malformed_secret_key(&bytes) {
                        return Err(error("migration-credential-boundary", "Interrupted credential-bearing JSON requires credential recovery before import", &path));
                    }
                    warnings.push(format!(
                        "Malformed JSON retained as original bytes: {relative}"
                    ));
                }
            }
        }
        out.push(LegacyFile {
            relative,
            hash: hash(&bytes),
            bytes: bytes.len() as u64,
            category: category.into(),
            modified_unix: meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .and_then(|duration| duration.as_secs().try_into().ok()),
        });
    }
    Ok(())
}
fn secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase().replace(['-', '_'], "");
    matches!(
        key.as_str(),
        "apikey"
            | "accesstoken"
            | "refreshtoken"
            | "authtoken"
            | "password"
            | "secret"
            | "authorization"
    )
}
fn malformed_secret_key(bytes: &[u8]) -> bool {
    // A truncated document can still contain complete credential keys. Decode
    // quoted keys separately, including Unicode escapes, before archiving bytes.
    let text = String::from_utf8_lossy(bytes);
    let keys = regex::Regex::new(r#""(?:\\.|[^"\\])*"\s*:"#).expect("constant JSON key pattern");
    let found = keys.find_iter(&text).any(|matched| {
        let token = matched.as_str().trim_end_matches(':').trim_end();
        serde_json::from_str::<String>(token).is_ok_and(|key| secret_key(&key))
    });
    found
}
fn secret_field(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, v)| {
            secret_key(key) && !v.is_null() && v.as_str() != Some("") || secret_field(v)
        }),
        Value::Array(values) => values.iter().any(secret_field),
        _ => false,
    }
}

pub(crate) fn credential_json(bytes: &[u8]) -> bool {
    match serde_json::from_slice::<Value>(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
    {
        Ok(value) => secret_field(&value),
        Err(_) => malformed_secret_key(bytes),
    }
}

pub fn prepare(
    id: &str,
    source: &Path,
    kind: &str,
    current: &Configuration,
) -> Result<MigrationPlan> {
    if !matches!(
        kind,
        "itm-manager" | "itm-engine" | "tcm-portable" | "tcm-state"
    ) {
        return Err(AppError::new(
            "migration-kind",
            "Unknown legacy source kind",
        ));
    }
    let source = paths::checked(source)?;
    let mut files = Vec::new();
    let mut warnings = Vec::new();
    walk(&source, &source, &mut files, &mut warnings)?;
    files.sort_by(|a, b| a.relative.cmp(&b.relative));
    if files.is_empty() {
        return Err(error(
            "migration-empty",
            "No recognized legacy data found",
            &source,
        ));
    }
    let mut roots = Vec::new();
    let mut locale = None;
    let mut interval = None;
    for file in files.iter().filter(|f| f.category == "configuration") {
        let data = read_snapshot(&source, file)?;
        let Ok(value) = serde_json::from_slice::<Value>(
            data.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&data),
        ) else {
            continue;
        };
        if kind.starts_with("tcm-") {
            if let Some(raw) = value.get("interval_seconds") {
                let seconds = raw
                    .as_u64()
                    .filter(|n| (30..=86400).contains(n))
                    .ok_or_else(|| {
                        error(
                            "migration-check-interval",
                            "Legacy interval_seconds must be an integer between 30 and 86400",
                            &source.join(&file.relative),
                        )
                    })? as u32;
                if interval.is_some_and(|old| old != seconds) {
                    return Err(error(
                        "migration-check-interval",
                        "Legacy configuration files disagree on interval_seconds",
                        &source,
                    ));
                }
                interval = Some(seconds);
            }
        }
        if let Some(language) = value.get("language").and_then(Value::as_str) {
            locale = Some(language.into());
        }
        if let Some(group) = value.get("library_roots").and_then(Value::as_object) {
            for (key, space) in [("movies", Space::Movie), ("tv", Space::Tv)] {
                if let Some(values) = group.get(key).and_then(Value::as_array) {
                    for path in values.iter().filter_map(Value::as_str) {
                        add_root(&mut roots, path, Some(space.clone()), true);
                    }
                }
            }
        }
        if let Some(group) = value.get("library_roots").and_then(Value::as_array) {
            for root in group {
                if let Some(path) = root.get("path").and_then(Value::as_str) {
                    let space = match root
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_ascii_lowercase()
                        .as_str()
                    {
                        "movie" | "movies" => Some(Space::Movie),
                        "tv" | "series" => Some(Space::Tv),
                        _ => None,
                    };
                    if root
                        .get("kind")
                        .and_then(Value::as_str)
                        .is_some_and(|kind| kind.eq_ignore_ascii_case("mixed"))
                    {
                        let enabled = root
                            .get("enabled")
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        add_root(&mut roots, path, Some(Space::Movie), enabled);
                        add_root(&mut roots, path, Some(Space::Tv), enabled);
                        continue;
                    }
                    add_root(
                        &mut roots,
                        path,
                        space,
                        root.get("enabled")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    );
                }
            }
        }
        if let Some(group) = value.get("roots").and_then(Value::as_array) {
            for path in group.iter().filter_map(Value::as_str) {
                add_root(&mut roots, path, None, true);
            }
        }
    }
    for root in &roots {
        if root.state != "ready" {
            warnings.push(format!(
                "Root requires review: {} ({})",
                root.path, root.state
            ));
        }
    }
    warnings.push("Legacy running/paused jobs are retained as history and require a new preview before execution".into());
    warnings.push(
        "Old update snapshots and process/lock files are not authorization to update or resume"
            .into(),
    );
    let fingerprint = hash(&serde_json::to_vec(&(
        kind,
        source.to_string_lossy(),
        &files,
        current.revision,
    ))?);
    Ok(MigrationPlan {
        id: id.into(),
        source: source.to_string_lossy().into(),
        source_kind: kind.into(),
        fingerprint,
        configuration_revision: current.revision,
        incremental: interval.map(|interval_seconds| crate::incremental::IncrementalSettings {
            revision: 0,
            interval_seconds,
        }),
        files,
        roots,
        locale,
        warnings,
    })
}
fn add_root(roots: &mut Vec<LegacyRoot>, path: &str, space: Option<Space>, enabled: bool) {
    if roots.iter().any(|r| r.path == path && r.space == space) {
        return;
    }
    let state = if !enabled {
        "disabled"
    } else if space.is_none() {
        "unassigned"
    } else if paths::checked(Path::new(path)).is_err() {
        "unavailable-or-needs-mapping"
    } else {
        "ready"
    };
    roots.push(LegacyRoot {
        path: path.into(),
        space,
        enabled,
        state: state.into(),
    });
}
pub fn read_snapshot(source: &Path, file: &LegacyFile) -> Result<Vec<u8>> {
    let relative = Path::new(&file.relative);
    if relative.is_absolute()
        || relative
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(AppError::new(
            "migration-path",
            "Invalid relative snapshot path",
        ));
    }
    let path = paths::within(source, &source.join(relative))?;
    let meta = fs::metadata(&path).map_err(|e| error("migration-read", e, &path))?;
    if !meta.is_file() || meta.len() > FILE_LIMIT {
        return Err(error(
            "migration-file-limit",
            "File changed size or kind",
            &path,
        ));
    }
    let bytes = fs::read(&path).map_err(|e| error("migration-read", e, &path))?;
    let modified_unix = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|duration| duration.as_secs().try_into().ok());
    if bytes.len() as u64 != file.bytes
        || hash(&bytes) != file.hash
        || file.modified_unix.is_some() && modified_unix != file.modified_unix
    {
        return Err(error(
            "migration-source-changed",
            "Legacy source changed after review; prepare a new plan",
            &path,
        ));
    }
    Ok(bytes)
}
pub fn merged_configuration(
    plan: &MigrationPlan,
    current: &Configuration,
) -> Result<(Configuration, Vec<LegacyRoot>)> {
    if current.revision != plan.configuration_revision {
        return Err(AppError::new(
            "configuration-conflict",
            "Configuration changed after migration review",
        ));
    }
    let mut next = current.clone();
    let mut pending = Vec::new();
    for root in &plan.roots {
        if root.state != "ready" {
            pending.push(root.clone());
            continue;
        }
        let real = paths::checked(Path::new(&root.path))?;
        if next
            .roots
            .iter()
            .any(|r| Path::new(&r.path) == real && Some(&r.space) == root.space.as_ref())
        {
            continue;
        }
        if next.roots.iter().any(|r| {
            (real.starts_with(&r.path) || Path::new(&r.path).starts_with(&real))
                && !(Path::new(&r.path) == real && Some(&r.space) != root.space.as_ref())
        }) {
            let mut root = root.clone();
            root.state = "overlapping-root".into();
            pending.push(root);
            continue;
        }
        next.roots.push(LibraryRoot {
            id: hash(
                format!(
                    "{}:{}",
                    if root.space == Some(Space::Movie) {
                        "movie"
                    } else {
                        "tv"
                    },
                    real.to_string_lossy()
                )
                .as_bytes(),
            ),
            space: root
                .space
                .clone()
                .ok_or_else(|| AppError::new("migration-space", "Missing space"))?,
            path: real.to_string_lossy().into(),
        });
    }
    if let Some(language) = &plan.locale {
        next.locale = match language.as_str() {
            "zh-CN" => Locale::Simplified,
            "zh-Hant" => Locale::Traditional,
            "en-US" => Locale::English,
            "fr-FR" => Locale::French,
            "ru-RU" => Locale::Russian,
            "ja-JP" => Locale::Japanese,
            "es-ES" => Locale::Spanish,
            "th-TH" => Locale::Thai,
            _ => next.locale,
        };
    }
    next.revision = next
        .revision
        .checked_add(1)
        .ok_or_else(|| AppError::new("configuration-revision", "Revision exhausted"))?;
    Ok((next, pending))
}
pub fn normalized_preferences(files: &BTreeMap<String, Value>) -> Value {
    // Keep every legacy field and custom prompt. Native feature adapters consume
    // these names explicitly; unknown fields remain recoverable in the snapshot.
    let mut merged = serde_json::Map::new();
    for value in files.values() {
        if let Some(map) = value.as_object() {
            for (key, value) in map {
                merged.insert(key.clone(), value.clone());
            }
        }
    }
    Value::Object(merged)
}

/// Restore the original directory editor rows alongside scanner configuration.
/// The input is the verified archived settings, never an arbitrary UI payload.
pub(crate) fn tcm_folder_settings(
    plan: &MigrationPlan,
    files: &BTreeMap<String, Value>,
    current: &Configuration,
    next: &mut Configuration,
    pending: &mut Vec<LegacyRoot>,
    previous: Option<crate::folders::FolderSettings>,
) -> Result<Option<crate::folders::FolderSettings>> {
    use crate::folders::{from_configuration, FolderKind, FolderSource, MediaFolder};
    if !plan.source_kind.starts_with("tcm-") {
        return Ok(None);
    }
    let mut settings = None;
    for (path, value) in files {
        let Some(rows) = value.get("library_roots").and_then(Value::as_array) else {
            continue;
        };
        let configured = value
            .get("roots_configured")
            .and_then(Value::as_bool)
            .unwrap_or(!rows.is_empty());
        let candidate = (configured, rows);
        if settings.is_some_and(|old| old != candidate) {
            return Err(
                AppError::new("migration-folders-conflict", "旧配置中的媒体目录不一致").at(path),
            );
        }
        settings = Some(candidate);
    }
    let Some((configured, rows)) = settings else {
        return Ok(None);
    };
    // A saved but unconfirmed draft never activates a scan scope on upgrade.
    if !configured {
        next.roots = current.roots.clone();
        *pending = plan
            .roots
            .iter()
            .cloned()
            .map(|mut root| {
                root.state = "not-configured".into();
                root
            })
            .collect();
        return Ok(None);
    }
    if rows.len() > 256 {
        return Err(AppError::new("invalid-folders", "媒体目录不能超过 256 个"));
    }
    let mut folders = from_configuration(current, previous);
    next.roots = current.roots.clone();
    let mut seen = std::collections::BTreeSet::new();
    for row in rows {
        let original = row.get("path").and_then(Value::as_str).unwrap_or("");
        if ["path", "name", "kind", "source"]
            .iter()
            .any(|field| row.get(field).is_some_and(|value| !value.is_string()))
            || row.get("enabled").is_some_and(|value| !value.is_boolean())
        {
            return Err(
                AppError::new("migration-folders-invalid", "旧媒体目录字段类型无效").at(original),
            );
        }
        if original.is_empty()
            || original.len() > 32768
            || original.chars().any(char::is_control)
            || !seen.insert(original.trim_end_matches(['\\', '/']).to_lowercase())
        {
            return Err(
                AppError::new("migration-folders-invalid", "旧媒体目录路径无效或重复").at(original),
            );
        }
        let enabled = row.get("enabled").and_then(Value::as_bool).unwrap_or(false);
        let kind = match row
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "movie" | "movies" => FolderKind::Movies,
            "tv" | "series" => FolderKind::Tv,
            "mixed" => FolderKind::Mixed,
            _ => FolderKind::Auto,
        };
        let path = match paths::configured_directory(Path::new(original)) {
            Ok(path) => path.to_string_lossy().into_owned(),
            // Disabled foreign-platform paths are presentation data. Enabled
            // unmapped paths remain explicitly pending and cannot be scanned.
            Err(_) if !enabled => original.to_string(),
            Err(_) => {
                for space in kind.spaces() {
                    if !pending
                        .iter()
                        .any(|root| root.path == original && root.space == Some(space.clone()))
                    {
                        pending.push(LegacyRoot {
                            path: original.into(),
                            space: Some(space),
                            enabled,
                            state: "unavailable-or-needs-mapping".into(),
                        });
                    }
                }
                continue;
            }
        };
        if folders.folders.iter().any(|folder| folder.path == path) {
            continue; // Existing destination settings take precedence.
        }
        if enabled
            && next.roots.iter().any(|root| {
                root.path != path
                    && (Path::new(&root.path).starts_with(&path)
                        || Path::new(&path).starts_with(&root.path))
            })
        {
            continue; // merged_configuration already records the overlap.
        }
        let name = row
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if name.len() > 1024 || name.chars().any(char::is_control) {
            return Err(
                AppError::new("migration-folders-invalid", "旧媒体目录名称无效").at(original),
            );
        }
        let folder = MediaFolder {
            id: hash(path.as_bytes()),
            path: path.clone(),
            name,
            kind,
            source: if row
                .get("source")
                .and_then(Value::as_str)
                .is_some_and(|source| source.eq_ignore_ascii_case("auto"))
            {
                FolderSource::Auto
            } else {
                FolderSource::Manual
            },
            enabled,
        };
        if enabled {
            for space in folder.kind.spaces() {
                if !next
                    .roots
                    .iter()
                    .any(|root| root.path == path && root.space == space)
                {
                    next.roots.push(LibraryRoot {
                        id: hash(format!("{}:{space:?}", folder.id).as_bytes()),
                        path: path.clone(),
                        space,
                    });
                }
            }
            pending.retain(|root| root.path != original);
        }
        folders.folders.push(folder);
    }
    if folders.folders.len() > 256 {
        return Err(AppError::new("invalid-folders", "媒体目录不能超过 256 个"));
    }
    folders.revision = next.revision;
    Ok(Some(folders))
}
pub fn source_path(plan: &MigrationPlan) -> PathBuf {
    PathBuf::from(&plan.source)
}

pub(crate) fn tcm_application_settings(
    files: &BTreeMap<String, Value>,
) -> Result<Option<crate::lifecycle::Settings>> {
    let mut values = BTreeMap::new();
    for (path, value) in files {
        for field in ["auto_start", "auto_start_configured", "silent_start"] {
            let Some(raw) = value.get(field) else {
                continue;
            };
            let enabled = raw.as_bool().ok_or_else(|| {
                AppError::new("migration-application-settings", "旧应用设置字段类型无效").at(path)
            })?;
            if values
                .insert(field, enabled)
                .is_some_and(|old| old != enabled)
            {
                return Err(AppError::new(
                    "migration-application-settings",
                    "旧配置中的应用设置不一致",
                )
                .at(path));
            }
        }
    }
    if values.is_empty() {
        return Ok(None);
    }
    Ok(Some(crate::lifecycle::Settings {
        revision: 1,
        launch_at_login: values.get("auto_start").copied().unwrap_or(false),
        start_hidden: values.get("silent_start").copied().unwrap_or(false),
        ..Default::default()
    }))
}
