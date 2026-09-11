//! Emby 4.9.5 physical-root discovery. The server database is opened read-only.
use crate::{paths, AppError, Result, Space};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
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
    pub name: String,
    pub server_path: String,
    pub local_path: Option<String>,
    pub spaces: Vec<Space>,
    pub movie_evidence: u32,
    pub series_evidence: u32,
    pub episode_evidence: u32,
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
WITH RECURSIVE candidates(Id,Name,Path) AS (
 SELECT child.Id,child.Name,child.Path FROM MediaItems child
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
 SUM(CASE WHEN i.type=5 THEN 1 ELSE 0 END)
FROM candidates c JOIN tree ON tree.RootId=c.Id JOIN MediaItems i ON i.Id=tree.Id
GROUP BY c.Id,c.Name,c.Path ORDER BY c.Id LIMIT 10001
"#;
fn probe(root: &Path) -> (bool, bool, Vec<String>) {
    let started = Instant::now();
    let mut stack = vec![root.to_path_buf()];
    let (mut movie, mut tv, mut count) = (false, false, 0usize);
    let mut issues = Vec::new();
    while let Some(path) = stack.pop() {
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
    (movie, tv, issues)
}
pub fn discover(
    data: &Path,
    version: &str,
    mappings: &[PathMapping],
) -> Result<Vec<DiscoveredLibrary>> {
    if version != "4.9.5.0" {
        return Err(AppError::new(
            "emby-database-version",
            "Verify the supported Emby version before database discovery",
        ));
    }
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
    connection.progress_handler(
        10000,
        Some(move || started.elapsed() > Duration::from_secs(15)),
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
        ))
    })?;
    let rows = rows
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| failure(e).at(database.display()))?;
    drop(statement);
    drop(connection);
    let mut output = Vec::new();
    for (id, name, server_path, movies, series, episodes, videos) in rows {
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
                let (a, b, extra) = probe(path);
                movie = a;
                tv = b;
                issues.extend(extra);
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
            name,
            server_path,
            local_path: local.map(|p| p.to_string_lossy().into()),
            spaces,
            movie_evidence: movies,
            series_evidence: series,
            episode_evidence: episodes,
            state: state.into(),
            issues,
        });
    }
    Ok(output)
}
