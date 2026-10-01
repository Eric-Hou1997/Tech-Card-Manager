use super::*;
use crate::emby_libraries::{self, DiscoveredLibrary, MappingSettings};

fn key(data: &str, mappings: &MappingSettings) -> Result<String> {
    Ok(format!(
        "root-discovery-{}",
        hash(&serde_json::to_vec(&(data, &mappings.mappings))?)
    ))
}
fn legacy_snapshot(data: &str, mappings: &MappingSettings) -> Result<Vec<DiscoveredLibrary>> {
    use std::io::Read;
    let root = Path::new(data);
    let path = root.join("custom-tech-specs/manager-root-discovery.json");
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(error) => return Err(AppError::new("legacy-discovery-read", error).at(path.display())),
        Ok(_) => {}
    }
    let path = paths::within(root, &path)?;
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .map_err(|e| AppError::new("legacy-discovery-read", e).at(path.display()))?
        .take((8 << 20) + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| AppError::new("legacy-discovery-read", e).at(path.display()))?;
    if bytes.len() > 8 << 20 {
        return Err(
            AppError::new("legacy-discovery-size", "旧媒体目录发现文件过大").at(path.display()),
        );
    }
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Row {
        #[serde(default)]
        id: serde_json::Value,
        #[serde(default)]
        parent_id: serde_json::Value,
        #[serde(default)]
        name: String,
        #[serde(default)]
        path: String,
        #[serde(default)]
        kind: String,
        #[serde(default)]
        included: bool,
        #[serde(default)]
        online: bool,
        #[serde(default)]
        imdb_video_count: u32,
        #[serde(default)]
        series_count: u32,
        #[serde(default)]
        episode_count: u32,
        #[serde(default)]
        evidence: String,
    }
    #[derive(serde::Deserialize)]
    struct Snapshot {
        libraries: Vec<Row>,
    }
    let snapshot: Snapshot =
        serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))
            .map_err(|e| AppError::new("legacy-discovery-invalid", e).at(path.display()))?;
    if snapshot.libraries.len() > 10000 {
        return Err(
            AppError::new("legacy-discovery-size", "旧媒体目录发现记录过多").at(path.display()),
        );
    }
    let id = |value: serde_json::Value| match value {
        serde_json::Value::String(s) => Some(s),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    };
    let mut rows = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for row in snapshot.libraries {
        if !row.included || row.path.trim().is_empty() {
            continue;
        }
        let server_path = row.path.trim().to_string();
        let spaces = match row.kind.to_ascii_lowercase().as_str() {
            "movies" | "movie" => vec![Space::Movie],
            "tv" => vec![Space::Tv],
            "mixed movie/tv" | "mixed" => vec![Space::Movie, Space::Tv],
            _ => {
                return Err(
                    AppError::new("legacy-discovery-kind", "旧媒体目录分类无效").at(&server_path)
                )
            }
        };
        if server_path.len() > 32768
            || server_path.chars().any(char::is_control)
            || !seen.insert(server_path.trim_end_matches(['\\', '/']).to_lowercase())
        {
            return Err(
                AppError::new("legacy-discovery-path", "旧媒体目录路径无效或重复").at(&server_path),
            );
        }
        // Preserve the cached online observation; do not probe media/NFOs while
        // restoring this presentation snapshot. Real scans validate paths anew.
        let local_path = if row.online {
            emby_libraries::mapped_path(&server_path, &mappings.mappings)
                .ok()
                .map(|p| p.to_string_lossy().into())
        } else {
            None
        };
        let state = if local_path.is_none() {
            "offline-or-needs-mapping"
        } else if spaces.len() > 1 {
            "mixed-requires-scope-review"
        } else {
            "ready"
        };
        rows.push(DiscoveredLibrary {
            id: id(row.id).unwrap_or_else(|| hash(server_path.as_bytes())),
            parent_id: id(row.parent_id),
            name: row.name,
            server_path,
            local_path,
            spaces,
            movie_evidence: row.imdb_video_count,
            series_evidence: row.series_count,
            episode_evidence: row.episode_count,
            evidence: row.evidence,
            probe_nfos: None,
            state: state.into(),
            issues: vec![],
        });
    }
    Ok(rows)
}
impl Store {
    /// Cached discovery is presentation data, not a configured scan scope.
    pub fn discovered_libraries(&self, data: &str) -> Result<Vec<DiscoveredLibrary>> {
        let mappings = self.path_mappings()?;
        let value = self.preferences(&key(data, &mappings)?)?;
        if value.is_null() || value.as_object().is_some_and(|v| v.is_empty()) {
            legacy_snapshot(data, &mappings)
        } else {
            Ok(serde_json::from_value(value)?)
        }
    }
    pub fn discover_libraries(&self, data: &str) -> Result<Vec<DiscoveredLibrary>> {
        self.discover_libraries_cancellable(
            data,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
    }
    pub(crate) fn discover_libraries_cancellable(
        &self,
        data: &str,
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<Vec<DiscoveredLibrary>> {
        emby_libraries::check_cancelled(&cancelled)?;
        let mappings = self.path_mappings()?;
        let cache_key = key(data, &mappings)?;
        let cached = self.preferences(&cache_key)?;
        let previous = if cached.is_null() || cached.as_object().is_some_and(|v| v.is_empty()) {
            // Like the original Get-PreviousLibraryMap, an unusable historical
            // classification must not prevent an explicit fresh database query.
            // Normal snapshot reads still report the damaged legacy file.
            legacy_snapshot(data, &mappings).unwrap_or_default()
        } else {
            serde_json::from_value(cached)?
        };
        let rows = emby_libraries::discover_with_control(
            Path::new(data),
            &mappings.mappings,
            &previous,
            cancelled.clone(),
        )?;
        // Save only a complete query. Failures leave the last usable snapshot
        // intact, and snapshots cannot leak across installations or mappings.
        emby_libraries::check_cancelled(&cancelled)?;
        self.save_preference(&cache_key, &serde_json::to_value(&rows)?)?;
        Ok(rows)
    }
}
