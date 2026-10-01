//! Original Manager physical-root query. The server database is opened read-only.
use crate::{paths, AppError, Result, Space};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use ts_rs::TS;
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct PathMapping {
    pub server_prefix: String,
    pub local_root: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
pub struct MappingSettings {
    pub revision: u32,
    pub mappings: Vec<PathMapping>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct DiscoveredLibrary {
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub name: String,
    pub server_path: String,
    pub local_path: Option<String>,
    pub spaces: Vec<Space>,
    pub movie_evidence: u32,
    pub series_evidence: u32,
    pub episode_evidence: u32,
    #[serde(default)]
    pub evidence: String,
    #[serde(default)]
    pub probe_nfos: Option<u32>,
    pub state: String,
    pub issues: Vec<String>,
}
fn failure(message: impl ToString) -> AppError {
    AppError::new("emby-library-discovery", message)
}
fn parts(value: &str) -> Result<(bool, Vec<String>)> {
    let windows = value.starts_with("\\\\") || value.as_bytes().get(1) == Some(&b':');
    let value = if windows {
        value.replace('\\', "/")
    } else {
        value.to_owned()
    };
    if value.starts_with("//?/")
        || value.starts_with("//./")
        || (!windows && !value.starts_with('/'))
        || value.contains('\0')
    {
        return Err(AppError::new(
            "path-mapping",
            "Server path must be an ordinary absolute path",
        ));
    }
    let values: Vec<_> = value
        .split('/')
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .collect();
    if values.is_empty()
        || values.iter().any(|v| v == "." || v == "..")
        || windows
            && value.as_bytes().get(1) == Some(&b':')
            && value.as_bytes().get(2) != Some(&b'/')
    {
        return Err(AppError::new("path-mapping", "Ambiguous server path"));
    }
    Ok((windows, values))
}
pub fn mapped_path(server: &str, mappings: &[PathMapping]) -> Result<PathBuf> {
    let (windows, path) = parts(server)?;
    let mut best: Option<(usize, PathBuf)> = None;
    for mapping in mappings {
        let (style, prefix) = parts(&mapping.server_prefix)?;
        if windows != style
            || prefix.len() > path.len()
            || !prefix.iter().zip(&path).all(|(a, b)| {
                if windows {
                    a.to_lowercase() == b.to_lowercase()
                } else {
                    a == b
                }
            })
        {
            continue;
        }
        let local = paths::checked(Path::new(&mapping.local_root))?;
        if !local.is_dir() {
            return Err(AppError::new(
                "path-mapping",
                "Mapping destination must be a directory",
            ));
        }
        let mut candidate = local;
        for component in &path[prefix.len()..] {
            candidate.push(component);
        }
        if let Some((length, _)) = &best {
            if *length == prefix.len() {
                return Err(AppError::new(
                    "path-mapping",
                    "Equally specific mappings conflict",
                ));
            }
            if *length > prefix.len() {
                continue;
            }
        }
        best = Some((prefix.len(), candidate));
    }
    if let Some((_, path)) = best {
        return Ok(path);
    }
    if windows != cfg!(windows) {
        return Err(AppError::new(
            "path-mapping-required",
            "Server path needs an explicit local mapping",
        ));
    }
    Ok(PathBuf::from(server))
}
const QUERY: &str = r#"
WITH RECURSIVE candidates(Id,Name,Path,ParentId) AS (
 SELECT child.Id,child.Name,child.Path,child.ParentId FROM MediaItems child
 LEFT JOIN MediaItems parent ON parent.Id=child.ParentId
 WHERE child.type=3 AND child.Path IS NOT NULL AND trim(child.Path)<>''
 AND (child.ParentId=2 OR parent.Path IS NULL OR trim(parent.Path)='')
), tree(RootId,Id) AS (
 SELECT Id,Id FROM candidates UNION
 SELECT tree.RootId,child.Id FROM MediaItems child JOIN tree ON child.ParentId=tree.Id
 WHERE child.type IN(3,5,6,7,8)
)
SELECT c.Id,c.Name,c.Path,
 SUM(CASE WHEN i.type=5 AND lower(COALESCE(i.ProviderIds,'')) LIKE '%imdb=tt%'
 AND(i.ExtraType IS NULL OR trim(CAST(i.ExtraType AS TEXT))='' OR CAST(i.ExtraType AS TEXT)='0') THEN 1 ELSE 0 END),
 SUM(CASE WHEN i.type=6 THEN 1 ELSE 0 END),SUM(CASE WHEN i.type=8 THEN 1 ELSE 0 END),
 SUM(CASE WHEN i.type=5 THEN 1 ELSE 0 END),c.ParentId
FROM candidates c JOIN tree ON tree.RootId=c.Id JOIN MediaItems i ON i.Id=tree.Id
GROUP BY c.Id,c.Name,c.Path,c.ParentId ORDER BY c.Id LIMIT 10001
"#;
fn probe(root: &Path, cancelled: &AtomicBool) -> (bool, bool, Vec<String>, u32) {
    let started = Instant::now();
    let mut stack = vec![root.to_path_buf()];
    let (mut movie, mut tv, mut count) = (false, false, 0usize);
    let mut issues = Vec::new();
    let mut checked_nfos = 0;
    while let Some(path) = stack.pop() {
        if cancelled.load(Ordering::SeqCst) {
            break;
        }
        count += 1;
        if count > 10000 || started.elapsed() > Duration::from_secs(5) {
            issues.push("nfo-evidence-probe-incomplete".into());
            break;
        }
        let result = (|| -> Result<()> {
            let path = paths::within(root, &path)?;
            if path.is_dir() {
                for entry in std::fs::read_dir(&path).map_err(failure)? {
                    stack.push(entry.map_err(failure)?.path());
                }
                return Ok(());
            }
            if !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("nfo"))
            {
                return Ok(());
            }
            let library = crate::LibraryRoot {
                id: "probe".into(),
                path: root.to_string_lossy().into(),
                space: Space::Movie,
            };
            checked_nfos += 1;
            let (_, bytes) = crate::library::read_bytes(&library, &path)?;
            let item = crate::library::parse(&library, &path, &bytes)?;
            movie |= item.kind == "Movie" && !item.imdb.is_empty();
            tv |= matches!(item.kind.as_str(), "Series" | "Season" | "Episode");
            Ok(())
        })();
        if let Err(error) = result {
            if issues.len() < 20 {
                issues.push(format!("{}: {}", error.code, path.display()));
            }
        }
        if movie && tv {
            break;
        }
    }
    (movie, tv, issues, checked_nfos)
}
pub fn discover(data: &Path, mappings: &[PathMapping]) -> Result<Vec<DiscoveredLibrary>> {
    discover_with_previous(data, mappings, &[])
}
pub fn discover_with_previous(
    data: &Path,
    mappings: &[PathMapping],
    previous: &[DiscoveredLibrary],
) -> Result<Vec<DiscoveredLibrary>> {
    discover_with_control(data, mappings, previous, Arc::new(AtomicBool::new(false)))
}
pub fn discover_with_control(
    data: &Path,
    mappings: &[PathMapping],
    previous: &[DiscoveredLibrary],
    cancelled: Arc<AtomicBool>,
) -> Result<Vec<DiscoveredLibrary>> {
    check_cancelled(&cancelled)?;
    if mappings.len() > 100 {
        return Err(AppError::new("path-mapping", "Too many path mappings"));
    }
    let data = paths::checked(data)?;
    let database = paths::within(&data, &data.join("data/library.db"))?;
    let connection = Connection::open_with_flags(
        &database,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| failure(e).at(database.display()))?;
    connection.busy_timeout(Duration::from_secs(2))?;
    connection.pragma_update(None, "query_only", true)?;
    let started = Instant::now();
    let query_cancelled = cancelled.clone();
    connection.progress_handler(
        10000,
        Some(move || {
            query_cancelled.load(Ordering::SeqCst) || started.elapsed() > Duration::from_secs(15)
        }),
    );
    let mut statement = connection
        .prepare(QUERY)
        .map_err(|e| failure(e).at(database.display()))?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?.to_string(),
            row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            row.get::<_, String>(2)?,
            row.get::<_, u32>(3)?,
            row.get::<_, u32>(4)?,
            row.get::<_, u32>(5)?,
            row.get::<_, u32>(6)?,
            row.get::<_, Option<i64>>(7)?.map(|id| id.to_string()),
        ))
    })?;
    let rows = rows
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| failure(e).at(database.display()));
    check_cancelled(&cancelled)?;
    let rows = rows?;
    drop(statement);
    drop(connection);
    let mut output = Vec::new();
    for (id, name, server_path, movies, series, episodes, videos, parent_id) in rows {
        check_cancelled(&cancelled)?;
        if started.elapsed() > Duration::from_secs(30) {
            return Err(failure(
                "Discovery exceeded its time budget; narrow the Emby library scope",
            ));
        }
        if output.len() >= 10000 {
            return Err(failure("Too many Emby physical roots"));
        }
        let (mut movie, mut tv) = (movies > 0, series > 0 || episodes > 0);
        let mut issues = Vec::new();
        let mut evidence = Vec::new();
        if movies > 0 {
            evidence.push(format!("IMDb-video={movies}"));
        }
        if series > 0 {
            evidence.push(format!("Series={series}"));
        }
        if episodes > 0 {
            evidence.push(format!("Episode={episodes}"));
        }
        let mut probe_nfos = None;
        let local = mapped_path(&server_path, mappings).and_then(|p| paths::checked(&p));
        let local = match local {
            Ok(path) if path.is_dir() => Some(path),
            Ok(_) => {
                issues.push("not-a-directory".into());
                None
            }
            Err(error) => {
                issues.push(error.code);
                None
            }
        };
        if !movie && !tv && videos > 0 {
            if let Some(path) = &local {
                let (a, b, extra, checked) = probe(path, &cancelled);
                movie = a;
                tv = b;
                issues.extend(extra);
                probe_nfos = Some(checked);
                if a {
                    evidence.push("NFO=movie".into());
                }
                if b {
                    evidence.push("NFO=tv".into());
                }
                if !a && !b {
                    evidence.push(format!("NFO-probe={checked}"));
                }
            }
        }
        if evidence.is_empty() {
            evidence.push(format!("video-items={videos}"));
        }
        // The original reader retains proven classification while a root is
        // offline. Match the exact server path; a reused database ID is not
        // evidence that a different physical root has the same media kind.
        if !movie && !tv && local.is_none() {
            if let Some(old) = previous.iter().find(|old| old.server_path == server_path) {
                movie = old.spaces.contains(&Space::Movie);
                tv = old.spaces.contains(&Space::Tv);
            }
        }
        let mut spaces = Vec::new();
        if movie {
            spaces.push(Space::Movie);
        }
        if tv {
            spaces.push(Space::Tv);
        }
        let state = if local.is_none() {
            "offline-or-needs-mapping"
        } else if spaces.is_empty() {
            "unclassified"
        } else if spaces.len() > 1 {
            "mixed-requires-scope-review"
        } else {
            "ready"
        };
        output.push(DiscoveredLibrary {
            id,
            parent_id,
            name,
            server_path,
            local_path: local.map(|p| p.to_string_lossy().into()),
            spaces,
            movie_evidence: movies,
            series_evidence: series,
            episode_evidence: episodes,
            evidence: evidence.join(", "),
            probe_nfos,
            state: state.into(),
            issues,
        });
    }
    check_cancelled(&cancelled)?;
    Ok(output)
}
pub(crate) fn check_cancelled(cancelled: &AtomicBool) -> Result<()> {
    if cancelled.load(Ordering::SeqCst) {
        Err(AppError::new("discovery-cancelled", "任务已取消"))
    } else {
        Ok(())
    }
}
