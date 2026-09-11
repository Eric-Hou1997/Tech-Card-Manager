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
            match serde_json::from_slice::<Value>(&bytes) {
                Ok(value) => {
                    if secret_field(&value) {
                        return Err(error("migration-credential-boundary","Credential-bearing JSON needs native credential migration before import",&path));
                    }
                }
                Err(_) => warnings.push(format!(
                    "Malformed JSON retained as original bytes: {relative}"
                )),
            }
        }
        out.push(LegacyFile {
            relative,
            hash: hash(&bytes),
            bytes: bytes.len() as u64,
            category: category.into(),
        });
    }
    Ok(())
}
fn secret_field(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, v)| {
            let key = key.to_ascii_lowercase().replace(['-', '_'], "");
            matches!(
                key.as_str(),
                "apikey" | "accesstoken" | "password" | "secret" | "authorization"
            ) && v.as_str().is_some_and(|s| !s.is_empty())
                || secret_field(v)
        }),
        Value::Array(values) => values.iter().any(secret_field),
        _ => false,
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
    for file in files.iter().filter(|f| f.category == "configuration") {
        let data = read_snapshot(&source, file)?;
        let Ok(value) = serde_json::from_slice::<Value>(&data) else {
            continue;
        };
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
        files,
        roots,
        locale,
        warnings,
    })
}
fn add_root(roots: &mut Vec<LegacyRoot>, path: &str, space: Option<Space>, enabled: bool) {
    if roots.iter().any(|r| r.path == path) {
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
    if bytes.len() as u64 != file.bytes || hash(&bytes) != file.hash {
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
        if next.roots.iter().any(|r| Path::new(&r.path) == real) {
            continue;
        }
        if next
            .roots
            .iter()
            .any(|r| real.starts_with(&r.path) || Path::new(&r.path).starts_with(&real))
        {
            let mut root = root.clone();
            root.state = "overlapping-root".into();
            pending.push(root);
            continue;
        }
        next.roots.push(LibraryRoot {
            id: hash(real.to_string_lossy().as_bytes()),
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
pub fn source_path(plan: &MigrationPlan) -> PathBuf {
    PathBuf::from(&plan.source)
}
