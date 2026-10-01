use super::*;
use crate::folders::{FolderKind, FolderSettings, FolderSource, MediaFolder};
use crate::migration::MigrationReceipt;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;

#[derive(Deserialize)]
struct LegacyCatalog {
    #[serde(rename = "generatedAt")]
    generated_at: String,
    count: usize,
    items: Vec<LegacyCatalogItem>,
}

#[derive(Deserialize)]
struct LegacyCatalogItem {
    path: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    title: String,
    #[serde(default, rename = "originalTitle")]
    original_title: String,
    #[serde(default, rename = "showTitle")]
    show_title: String,
    #[serde(default, rename = "seriesTitle")]
    series_title: String,
    #[serde(default)]
    year: String,
    #[serde(default)]
    season: String,
    #[serde(default)]
    episode: String,
    #[serde(default)]
    imdb: String,
    #[serde(default, rename = "libraryKind")]
    library_kind: String,
    #[serde(default, rename = "hasTechnicalSpecs")]
    has_technical_specs: bool,
    #[serde(default, rename = "tagCount")]
    tag_count: usize,
    #[serde(default)]
    tags: Vec<Tag>,
    #[serde(default)]
    specs: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    error: String,
}

fn legacy_path(value: &str) -> String {
    value
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

fn legacy_within(path: &str, root: &str) -> bool {
    let path = legacy_path(path);
    let root = legacy_path(root);
    path == root
        || path
            .strip_prefix(&root)
            .is_some_and(|relative| relative.starts_with('\\'))
}

fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes)
}

fn recovered_kind(value: &str) -> FolderKind {
    let value = value.trim().to_ascii_lowercase();
    if value.contains("mixed") {
        FolderKind::Mixed
    } else if value.contains("tv") || value.contains("series") {
        FolderKind::Tv
    } else if value.contains("movie") {
        FolderKind::Movies
    } else {
        FolderKind::Auto
    }
}

/// v4.1.0 rebuilt a missing portable settings file from its last complete
/// index summary. Restore only that directory choice; interval and locale keep
/// their existing defaults, and any new-stack configuration wins.
fn recover_folders_from_summary(
    summary_bytes: &[u8],
    current: &Configuration,
) -> Result<Option<(Configuration, FolderSettings)>> {
    if current.revision != 0 || !current.roots.is_empty() {
        return Ok(None);
    }
    let summary: serde_json::Value =
        serde_json::from_slice(strip_bom(summary_bytes)).map_err(|e| {
            AppError::new("migration-summary-invalid", e).at("manager-index-summary.json")
        })?;
    if summary
        .get("generatedAt")
        .and_then(serde_json::Value::as_str)
        .is_none_or(|value| value.trim().is_empty())
        || !summary
            .get("scanStats")
            .is_some_and(serde_json::Value::is_object)
        || !summary
            .get("items")
            .is_some_and(serde_json::Value::is_object)
    {
        return Ok(None);
    }
    let invalid = |message: &str| AppError::new("migration-summary-invalid", message);
    let libraries = match summary.get("libraries") {
        None => Vec::new(),
        Some(value) => value
            .as_array()
            .ok_or_else(|| invalid("Original Manager libraries are invalid"))?
            .iter()
            .map(|row| {
                let row = row
                    .as_object()
                    .ok_or_else(|| invalid("Original Manager library is invalid"))?;
                for key in ["path", "name", "kind"] {
                    if row.get(key).is_some_and(|value| !value.is_string()) {
                        return Err(invalid("Original Manager library fields are invalid"));
                    }
                }
                Ok((
                    row.get("path")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(""),
                    row.get("name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(""),
                    row.get("kind")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or(""),
                ))
            })
            .collect::<Result<Vec<_>>>()?,
    };
    let rows = if libraries.iter().any(|(path, _, _)| !path.trim().is_empty()) {
        libraries
    } else {
        match summary.get("libraryRoots") {
            None => Vec::new(),
            Some(value) => value
                .as_array()
                .ok_or_else(|| invalid("Original Manager library roots are invalid"))?
                .iter()
                .map(|path| {
                    path.as_str()
                        .map(|path| (path, "", ""))
                        .ok_or_else(|| invalid("Original Manager library root is invalid"))
                })
                .collect::<Result<Vec<_>>>()?,
        }
    };
    if rows.is_empty() {
        return Ok(None);
    }
    if rows.len() > 256 {
        return Err(invalid("Original Manager has more than 256 media folders"));
    }
    let mut folders = Vec::with_capacity(rows.len());
    let mut seen = BTreeSet::new();
    for (path, name, kind) in rows {
        let original = path.trim().trim_matches('"');
        if original.is_empty() || original.chars().any(char::is_control) {
            return Err(invalid("Original Manager media folder path is invalid").at(original));
        }
        let path = paths::configured_directory(std::path::Path::new(original))?
            .to_string_lossy()
            .into_owned();
        let key = legacy_path(&path);
        if !seen.insert(key.clone())
            || seen.iter().any(|other| {
                other != &key && (legacy_within(&key, other) || legacy_within(other, &key))
            })
        {
            return Err(invalid("Original Manager media folders overlap or repeat").at(path));
        }
        let name = name.trim();
        if name.len() > 1024 || name.chars().any(char::is_control) {
            return Err(invalid("Original Manager media folder name is invalid").at(path));
        }
        let kind = recovered_kind(kind);
        let id = hash(path.as_bytes());
        folders.push(MediaFolder {
            id: id.clone(),
            path: path.clone(),
            name: if name.is_empty() {
                std::path::Path::new(&path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            } else {
                name.into()
            },
            kind: kind.clone(),
            source: FolderSource::Auto,
            enabled: true,
        });
    }
    let mut configuration = current.clone();
    for folder in &folders {
        for space in folder.kind.spaces() {
            configuration.roots.push(LibraryRoot {
                id: hash(format!("{}:{space:?}", folder.id).as_bytes()),
                space,
                path: folder.path.clone(),
            });
        }
    }
    Ok(Some((
        configuration,
        FolderSettings {
            revision: current.revision,
            folders,
        },
    )))
}

fn restore_catalog(
    summary_bytes: &[u8],
    catalog_bytes: &[u8],
    configuration: &Configuration,
) -> Result<Option<(String, Vec<MediaItem>)>> {
    let summary: serde_json::Value =
        serde_json::from_slice(strip_bom(summary_bytes)).map_err(|e| {
            AppError::new("migration-summary-invalid", e).at("manager-index-summary.json")
        })?;
    let invalid = |message: &str| AppError::new("migration-catalog-invalid", message);
    if summary.get("version").and_then(serde_json::Value::as_u64) != Some(5) {
        return Err(invalid("Original Manager index summary is not version 5"));
    }
    let roots = match summary.get("libraryRoots") {
        None => BTreeSet::new(),
        Some(value) => value
            .as_array()
            .and_then(|roots| {
                roots
                    .iter()
                    .map(|root| root.as_str().map(legacy_path))
                    .collect::<Option<BTreeSet<_>>>()
            })
            .ok_or_else(|| invalid("Original Manager library roots are invalid"))?,
    };
    // The old program itself refused to present a catalog whose configured
    // roots differed from its completed summary. Preserve that fail-closed rule.
    let configured: BTreeSet<_> = configuration
        .roots
        .iter()
        .map(|root| legacy_path(&root.path))
        .collect();
    if roots.is_empty() || configured != roots {
        return Ok(None);
    }
    let catalog: LegacyCatalog = serde_json::from_slice(strip_bom(catalog_bytes))
        .map_err(|e| AppError::new("migration-catalog-invalid", e).at("manager-catalog.json"))?;
    if catalog.generated_at.is_empty()
        || catalog.count != catalog.items.len()
        || summary
            .get("catalogCount")
            .and_then(serde_json::Value::as_u64)
            != u64::try_from(catalog.count).ok()
    {
        return Err(invalid(
            "Original Manager catalog does not match its completed summary",
        ));
    }
    let mut seen = BTreeSet::new();
    let mut restored = Vec::with_capacity(catalog.items.len());
    for row in catalog.items {
        if row.path.is_empty()
            || row.path.chars().any(char::is_control)
            || !seen.insert(legacy_path(&row.path))
            || !matches!(
                row.kind.as_str(),
                "Movie" | "Series" | "Season" | "Episode" | "InvalidNFO"
            )
            || row.has_technical_specs != !row.specs.is_empty()
            || row.tag_count != row.tags.len()
        {
            return Err(invalid("Original Manager catalog contains an invalid row").at(row.path));
        }
        // This is the exact v4 catalog partition: only a TV media type or an
        // explicit tv library marker enters the TV space.
        let space = if matches!(row.kind.as_str(), "Series" | "Season" | "Episode")
            || row.library_kind.eq_ignore_ascii_case("tv")
        {
            Space::Tv
        } else {
            Space::Movie
        };
        let root = configuration
            .roots
            .iter()
            .filter(|root| root.space == space && legacy_within(&row.path, &root.path))
            .max_by_key(|root| legacy_path(&root.path).len())
            .ok_or_else(|| {
                invalid("Original Manager catalog row is outside the restored media roots")
                    .at(&row.path)
            })?;
        let path = row.path;
        restored.push(MediaItem {
            // A restored display cache must be reparsed from NFO as soon as its
            // root is reachable; it never claims current parser/source identity.
            parser_revision: 0,
            id: hash(path.as_bytes()),
            root_id: root.id.clone(),
            space,
            path: path.clone(),
            source_hash: String::new(),
            title: row.title,
            original_title: row.original_title,
            show_title: if row.show_title.is_empty() {
                row.series_title
            } else {
                row.show_title
            },
            year: row.year,
            imdb: row.imdb,
            kind: row.kind,
            season: row.season,
            episode: row.episode,
            specs: row.specs,
            tags: row.tags,
            error: (!row.error.is_empty())
                .then(|| AppError::new("legacy-index-error", row.error).at(path)),
        });
    }
    Ok(Some((catalog.generated_at, restored)))
}

/// Original Windows builds write only under this current user's roaming Emby.
/// Validation builds and other platforms must not import real legacy state.
pub fn original_source(
    os: &str,
    identifier: &str,
    web: &Path,
    roaming: Option<&Path>,
) -> Result<Option<std::path::PathBuf>> {
    if os != "windows" || identifier.to_ascii_lowercase().contains("validation") {
        return Ok(None);
    }
    let Some(roaming) = roaming else {
        return Ok(None);
    };
    let root = roaming.join("Emby-Server");
    let expected = paths::configured_directory(&root.join("system/dashboard-ui"))?;
    if paths::checked(web)? != expected {
        return Ok(None);
    }
    Ok(Some(root))
}
fn read_fixed_capture(source: &Path, name: &str, limit: u64) -> Result<Option<(Vec<u8>, i64)>> {
    let path = source.join(name);
    let before = match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(AppError::new("migration-source-read", error).at(path.display())),
        Ok(meta) => meta,
    };
    if !before.is_file() || before.len() > limit {
        return Err(AppError::new(
            "migration-file-limit",
            "Original diagnostic source is not a bounded regular file",
        )
        .at(path.display()));
    }
    let real = paths::within(source, &path)?;
    let mut bytes = Vec::new();
    std::fs::File::open(&real)
        .and_then(|file| file.take(limit + 1).read_to_end(&mut bytes))
        .map_err(|error| AppError::new("migration-source-read", error).at(path.display()))?;
    let after = std::fs::symlink_metadata(&path)
        .map_err(|error| AppError::new("migration-source-read", error).at(path.display()))?;
    if bytes.len() as u64 > limit
        || bytes.len() as u64 != before.len()
        || !after.is_file()
        || after.len() != before.len()
        || after.modified().ok() != before.modified().ok()
        || paths::within(source, &path)? != real
    {
        return Err(AppError::new(
            "migration-source-changed",
            "Original diagnostic source changed while reading",
        )
        .at(path.display()));
    }
    let modified_unix = before
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|duration| duration.as_secs().try_into().ok())
        .ok_or_else(|| {
            AppError::new(
                "migration-source-time",
                "Original diagnostic modification time is unavailable",
            )
            .at(path.display())
        })?;
    Ok(Some((bytes, modified_unix)))
}
fn read_fixed(source: &Path, name: &str, limit: u64) -> Result<Option<Vec<u8>>> {
    Ok(read_fixed_capture(source, name, limit)?.map(|(bytes, _)| bytes))
}
impl Store {
    pub fn import_original_emby_diagnostics(
        &self,
        os: &str,
        identifier: &str,
        web: &Path,
        roaming: Option<&Path>,
    ) -> Result<Option<MigrationReceipt>> {
        let Some(root) = original_source(os, identifier, web, roaming)? else {
            return Ok(None);
        };
        self.capture_emby_diagnostics(&root)
    }
    fn capture_emby_diagnostics(&self, root: &Path) -> Result<Option<MigrationReceipt>> {
        if self.db()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM preferences WHERE key='manager-index-summary')",
            [],
            |row| row.get::<_, bool>(0),
        )? {
            return Ok(None);
        }
        // The selected Emby root is already verified. Locate the fixed child
        // without touching it: a receipt remains usable after that old child disappears.
        let source = paths::checked(root)?.join("programdata/custom-tech-specs");
        let source_text = source.to_str().ok_or_else(|| {
            AppError::new(
                "migration-encoding",
                "Original diagnostic path must be Unicode",
            )
        })?;
        let id = format!("emby-diagnostics-{}", hash(source_text.as_bytes()));
        let source_identity = hash(&serde_json::to_vec(&(
            "legacy-import",
            "tcm-state",
            source_text,
        ))?);
        {
            let db = self.db()?;
            if let Some(body) = db
                .query_row("SELECT result FROM operations WHERE id=?1 OR (fingerprint=?2 AND json_extract(result,'$.kind')='migration') ORDER BY rowid DESC LIMIT 1", params![id,source_identity], |row| {
                    row.get::<_, String>(0)
                })
                .optional()?
            {
                return match serde_json::from_str(&body)? {
                    OperationResult::Migration(receipt) if receipt.source == source_text => {
                        Ok(Some(receipt))
                    }
                    _ => Err(AppError::new(
                        "operation-conflict",
                        "Original diagnostic receipt belongs to different input",
                    )),
                };
            }
            if db.query_row(
                "SELECT EXISTS(SELECT 1 FROM preferences WHERE key='manager-index-summary')",
                [],
                |row| row.get::<_, bool>(0),
            )? {
                return Ok(None);
            }
        }
        // Establish original worker provenance before reading the potentially large summary.
        match std::fs::symlink_metadata(source.join("manager-index-summary.json")) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(AppError::new("migration-source-read", error).at(source.display()))
            }
            Ok(_) => {}
        }
        let engine =
            read_fixed(&source, "technical-specs-worker.ps1", 8 << 20)?.ok_or_else(|| {
                AppError::new(
                    "migration-source-unverified",
                    "Original Emby worker is missing",
                )
                .at(source.display())
            })?;
        if hash(&engine) != crate::legacy_process::BASELINE_ENGINE {
            return Err(AppError::new(
                "migration-source-unverified",
                "Original Emby worker does not match v4.1.0",
            )
            .at(source.display()));
        }
        let (summary, summary_modified) =
            read_fixed_capture(&source, "manager-index-summary.json", 64 << 20)?.ok_or_else(
                || {
                    AppError::new(
                        "migration-source-changed",
                        "Original summary disappeared during source verification",
                    )
                },
            )?;
        let mut files = vec![("manager-index-summary.json", summary, summary_modified)];
        if let Some((bytes, modified)) =
            read_fixed_capture(&source, "manager-xml-errors.json", 64 << 20)?
        {
            files.push(("manager-xml-errors.json", bytes, modified));
        }
        if let Some((bytes, modified)) =
            read_fixed_capture(&source, "manager-catalog.json", 64 << 20)?
        {
            files.push(("manager-catalog.json", bytes, modified));
        }
        for (name, bytes, _) in &files {
            if crate::migration::credential_json(bytes) {
                return Err(AppError::new(
                    "migration-credential-boundary",
                    "Credential-bearing JSON requires native credential migration",
                )
                .at(source.join(name).display()));
            }
        }
        let mut db = self.db()?;
        self.writable()?;
        let tx = db.transaction()?;
        if tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM preferences WHERE key='manager-index-summary')",
            [],
            |row| row.get::<_, bool>(0),
        )? {
            return Ok(None);
        }
        if tx.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE json_extract(body,'$.state') IN ('requested','running','paused','interrupted'))",[],|row| row.get::<_,bool>(0))? { return Err(AppError::new("active-task","Wait for the current scan before importing original diagnostics")); }
        if tx.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE id=?1 UNION ALL SELECT 1 FROM operations WHERE id=?1)",[&id],|row| row.get::<_,bool>(0))? { return Err(AppError::new("operation-conflict","Original diagnostic import already exists; reload its receipt")); }
        // Recheck both fixed files and the worker before committing any archive.
        if read_fixed(&source, "technical-specs-worker.ps1", 8 << 20)?.as_deref()
            != Some(engine.as_slice())
        {
            return Err(AppError::new(
                "migration-source-changed",
                "Original Emby worker changed",
            ));
        }
        for name in [
            "manager-index-summary.json",
            "manager-xml-errors.json",
            "manager-catalog.json",
        ] {
            let now = read_fixed_capture(&source, name, 64 << 20)?;
            let captured = files
                .iter()
                .find(|(path, _, _)| *path == name)
                .map(|(_, bytes, modified)| (bytes.as_slice(), *modified));
            if now
                .as_ref()
                .map(|(bytes, modified)| (bytes.as_slice(), *modified))
                != captured
            {
                return Err(AppError::new(
                    "migration-source-changed",
                    "Original diagnostic files changed before import",
                )
                .at(source.join(name).display()));
            }
        }
        jobs::require_no_diagnostic(&tx)?;
        let current = Self::folder_configuration(&tx)?;
        let has_folder_choice = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM preferences WHERE key='media-folders')",
            [],
            |row| row.get::<_, bool>(0),
        )?;
        let recovered = if has_folder_choice {
            None
        } else {
            recover_folders_from_summary(
                files
                    .iter()
                    .find(|(name, _, _)| *name == "manager-index-summary.json")
                    .expect("summary is required")
                    .1
                    .as_slice(),
                &current,
            )?
        };
        let configuration = if let Some((candidate, mut folders)) = recovered {
            let configuration = Self::write_configuration(&tx, &current, candidate)?;
            folders.revision = configuration.revision;
            tx.execute(
                "INSERT INTO preferences(key,body) VALUES('media-folders',?1)",
                [serde_json::to_string(&folders)?],
            )?;
            configuration
        } else {
            current
        };
        let catalog = files
            .iter()
            .find(|(name, _, _)| *name == "manager-catalog.json")
            .map(|(_, catalog, _)| {
                restore_catalog(
                    files
                        .iter()
                        .find(|(name, _, _)| *name == "manager-index-summary.json")
                        .expect("summary is required")
                        .1
                        .as_slice(),
                    catalog,
                    &configuration,
                )
            })
            .transpose()?
            .flatten();
        let mut catalog_restored = false;
        if tx.query_row("SELECT NOT EXISTS(SELECT 1 FROM items)", [], |row| {
            row.get::<_, bool>(0)
        })? {
            if let Some((generated_at, items)) = catalog {
                for item in items {
                    tx.execute(
                        "INSERT INTO items(id,root_id,seen_task,body) VALUES(?1,?2,?3,?4)",
                        params![item.id, item.root_id, id, serde_json::to_string(&item)?],
                    )?;
                }
                tx.execute("INSERT INTO preferences(key,body) VALUES('catalog-generated-at',?1) ON CONFLICT(key) DO UPDATE SET body=excluded.body", [serde_json::to_string(&generated_at)?])?;
                catalog_restored = true;
            }
        }
        let identities: Vec<_> = files
            .iter()
            .map(|(name, bytes, modified)| (*name, hash(bytes), *modified))
            .collect();
        let receipt = MigrationReceipt {
            id: id.clone(),
            source: source_text.into(),
            fingerprint: hash(&serde_json::to_vec(&(source_text, &identities))?),
            imported_files: files.len() as u32,
            imported_bytes: files
                .iter()
                .map(|(_, bytes, _)| bytes.len())
                .sum::<usize>()
                .to_string(),
            pending_roots: vec![],
            phase: if catalog_restored {
                "imported-with-readonly-catalog"
            } else {
                "imported-requires-adapter-validation"
            }
            .into(),
            configuration,
        };
        for (name, bytes, modified_unix) in files {
            tx.execute("INSERT INTO legacy_artifacts(import_id,path,category,sha256,body,modified_unix) VALUES(?1,?2,'legacy-state',?3,?4,?5)",params![id,name,hash(&bytes),bytes,modified_unix])?;
        }
        tx.execute(
            "INSERT INTO operations(id,fingerprint,result) VALUES(?1,?2,?3)",
            params![
                id,
                hash(&serde_json::to_vec(&(
                    "legacy-import",
                    "tcm-state",
                    source_text
                ))?),
                serde_json::to_string(&OperationResult::Migration(receipt.clone()))?
            ],
        )?;
        tx.commit()?;
        Ok(Some(receipt))
    }
}
