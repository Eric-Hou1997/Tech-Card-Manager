use super::*;
use crate::diagnostics::{IndexSummary, ScanStats, SummaryLibrary, XmlErrorReport, XmlErrorRow};
const KEY: &str = "manager-index-summary";
const PUBLICATION: &str = "manager-completed-publication";
#[derive(serde::Serialize, serde::Deserialize)]
struct CompletedPublication {
    roots: std::collections::BTreeSet<String>,
    fingerprint: String,
}

pub(super) fn capture_libraries(config: &Configuration) -> Vec<SummaryLibrary> {
    let mut libraries: Vec<SummaryLibrary> = Vec::new();
    for root in &config.roots {
        let kind = if root.space == Space::Tv {
            "TV"
        } else {
            "Movies"
        };
        if let Some(previous) = libraries
            .iter_mut()
            .find(|library| library.path == root.path)
        {
            if previous.kind != kind {
                previous.kind = "Mixed Movie/TV".into();
            }
        } else {
            libraries.push(SummaryLibrary {
                path: root.path.clone(),
                kind: kind.into(),
                online: paths::checked(Path::new(&root.path)).is_ok_and(|path| path.is_dir()),
            });
        }
    }
    libraries
}
const PENDING: &str = "manager-index-summary-pending";
#[derive(serde::Serialize, serde::Deserialize)]
struct PendingSummary {
    id: String,
    tasks: Vec<String>,
    libraries: Vec<SummaryLibrary>,
    results: std::collections::BTreeMap<String, ScanStats>,
    #[serde(default)]
    xml_errors: std::collections::BTreeMap<String, Vec<XmlErrorRow>>,
}
// Called inside the same transaction that queues all children of one original action.
// Only one action can be active. A cancelled prior action is replaced, never resumed.
pub(super) fn begin_group(
    db: &Connection,
    id: &str,
    tasks: &[Task],
    config: &Configuration,
) -> Result<()> {
    if tasks.is_empty() {
        return Ok(());
    }
    let pending = PendingSummary {
        id: id.into(),
        tasks: tasks.iter().map(|task| task.id.clone()).collect(),
        libraries: capture_libraries(config),
        results: Default::default(),
        xml_errors: Default::default(),
    };
    db.execute("INSERT INTO preferences(key,body) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET body=excluded.body", params![PENDING,serde_json::to_string(&pending)?])?;
    Ok(())
}
/// The first child that contains a physical path owns its single traversal.
/// Its targets include only roots already frozen in this same queued action.
pub(super) fn physical_roots(db: &Connection, task: &Task) -> Result<Vec<Vec<LibraryRoot>>> {
    let body = db
        .query_row(
            "SELECT body FROM preferences WHERE key=?1",
            [PENDING],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(body) = body else {
        return Ok(task.roots.iter().cloned().map(|root| vec![root]).collect());
    };
    let group: PendingSummary = serde_json::from_str(&body)?;
    if !group.tasks.contains(&task.id) {
        return Ok(task.roots.iter().cloned().map(|root| vec![root]).collect());
    }
    let members = group
        .tasks
        .iter()
        .map(|id| {
            let body: String =
                db.query_row("SELECT body FROM tasks WHERE id=?1", [id], |row| row.get(0))?;
            let member: Task = serde_json::from_str(&body)?;
            if member.id != *id
                || member.service_session != task.service_session
                || member.force_parse != task.force_parse
            {
                return Err(AppError::new(
                    "scan-group-conflict",
                    "Scan group membership changed",
                ));
            }
            Ok(member)
        })
        .collect::<Result<Vec<_>>>()?;
    let mut scans = Vec::new();
    for root in &task.roots {
        let owners: Vec<_> = members
            .iter()
            .filter(|member| {
                member
                    .roots
                    .iter()
                    .any(|candidate| candidate.path == root.path)
            })
            .collect();
        let first = owners
            .first()
            .ok_or_else(|| AppError::new("scan-group-conflict", "Physical root has no owner"))?;
        if first.id != task.id {
            if !group.results.contains_key(&first.id)
                || !matches!(first.state, TaskState::Completed | TaskState::Failed)
            {
                return Err(AppError::new(
                    "scan-group-incomplete",
                    "The physical root's owning task did not finish",
                ));
            }
            continue;
        }
        let targets = owners
            .iter()
            .flat_map(|member| member.roots.iter())
            .filter(|candidate| candidate.path == root.path)
            .cloned()
            .collect();
        scans.push(targets);
    }
    Ok(scans)
}
pub(super) fn finish_task(
    db: &Connection,
    task: &Task,
    generated_at: &str,
    stats: ScanStats,
    libraries: Vec<SummaryLibrary>,
    observed_paths: &[&str],
    xml_errors: Vec<XmlErrorRow>,
) -> Result<bool> {
    let pending = db
        .query_row(
            "SELECT body FROM preferences WHERE key=?1",
            [PENDING],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(body) = pending {
        let mut group: PendingSummary = serde_json::from_str(&body)?;
        if group.tasks.contains(&task.id) {
            for captured in &mut group.libraries {
                if observed_paths.contains(&captured.path.as_str()) {
                    if let Some(observed) = libraries
                        .iter()
                        .find(|library| library.path == captured.path)
                    {
                        captured.online = observed.online;
                    }
                }
            }
            group.results.insert(task.id.clone(), stats);
            group.xml_errors.insert(task.id.clone(), xml_errors);
            if group.tasks.iter().all(|id| group.results.contains_key(id)) {
                let mut combined = ScanStats::default();
                for stats in group.results.values() {
                    // Catalog-derived counts are recomputed once in save_summary.
                    combined.online_roots_scanned += stats.online_roots_scanned;
                    combined.nfo_seen += stats.nfo_seen;
                    combined.nfo_reparsed += stats.nfo_reparsed;
                    combined.xml_read_errors += stats.xml_read_errors;
                }
                let errors = group
                    .tasks
                    .iter()
                    .flat_map(|id| group.xml_errors.remove(id).unwrap_or_default())
                    .collect();
                save_summary(
                    db,
                    &group.id,
                    generated_at,
                    combined,
                    group.libraries,
                    errors,
                )?;
                db.execute("DELETE FROM preferences WHERE key=?1", [PENDING])?;
                return Ok(true);
            }
            db.execute(
                "UPDATE preferences SET body=?2 WHERE key=?1",
                params![PENDING, serde_json::to_string(&group)?],
            )?;
            return Ok(false);
        }
    }
    save_summary(db, &task.id, generated_at, stats, libraries, xml_errors)?;
    Ok(true)
}
pub(super) fn save_summary(
    db: &Connection,
    source_task: &str,
    generated_at: &str,
    mut scan_stats: ScanStats,
    libraries: Vec<SummaryLibrary>,
    xml_errors: Vec<XmlErrorRow>,
) -> Result<()> {
    let items = db
        .prepare("SELECT body FROM items ORDER BY id")?
        .query_map([], |row| row.get::<_, String>(0))?
        .map(|body| Ok(serde_json::from_str::<MediaItem>(&body?)?))
        .collect::<Result<Vec<_>>>()?;
    let catalog = crate::ui::summary(&items);
    // Bind the last complete card contents to the roots that produced them.
    // Normal in-progress scans retain this observation until they finish.
    let publication = CompletedPublication {
        roots: libraries
            .iter()
            .map(|library| library.path.clone())
            .collect(),
        fingerprint: crate::emby::index_fingerprint(&crate::emby::public_index(
            &items,
            String::new(),
        ))?,
    };
    db.execute("INSERT INTO preferences(key,body) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET body=excluded.body",params![PUBLICATION,serde_json::to_string(&publication)?])?;
    scan_stats.technical_specs_found =
        items.iter().filter(|item| !item.specs.is_empty()).count() as u64;
    scan_stats.web_eligible_specs_found = u64::from(catalog.web_eligible);
    scan_stats.episode_specs_excluded_from_web = u64::from(catalog.episodes_excluded);
    let library_roots = libraries
        .iter()
        .map(|library| library.path.clone())
        .collect();
    let summary = IndexSummary {
        generated_at: generated_at.into(),
        source_task: source_task.into(),
        indexed_titles: u64::from(catalog.displayable),
        scan_stats: Some(scan_stats),
        libraries,
        library_roots,
    };
    db.execute("INSERT INTO preferences(key,body) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET body=excluded.body",params![KEY,serde_json::to_string(&summary)?])?;
    let errors = XmlErrorReport {
        generated_at: generated_at.into(),
        count: xml_errors.len() as u64,
        errors: xml_errors,
    };
    db.execute("INSERT INTO preferences(key,body) VALUES('manager-xml-errors',?1) ON CONFLICT(key) DO UPDATE SET body=excluded.body", [serde_json::to_string(&errors)?])?;
    Ok(())
}
struct SummarySource {
    summary: IndexSummary,
    bytes: Vec<u8>,
    import: Option<String>,
    modified_unix: Option<i64>,
}
impl Store {
    /// The actual card file must match a completed scan of the current roots.
    /// No catalog deserialization or filesystem probes are needed on status polls.
    pub fn index_current(&self, actual: Option<&str>) -> Result<bool> {
        let Some(actual) = actual else {
            return Ok(false);
        };
        let db = self.db()?;
        let body: Option<String> = db
            .query_row(
                "SELECT body FROM preferences WHERE key=?1",
                [PUBLICATION],
                |row| row.get(0),
            )
            .optional()?;
        let Some(body) = body else { return Ok(false) };
        let completed: CompletedPublication = serde_json::from_str(&body)?;
        let config = Self::folder_configuration(&db)?;
        let roots: std::collections::BTreeSet<_> =
            config.roots.iter().map(|root| root.path.clone()).collect();
        Ok(!roots.is_empty() && roots == completed.roots && actual == completed.fingerprint)
    }

    pub fn index_xml_errors(&self) -> Result<Option<XmlErrorReport>> {
        self.diagnostic_index()?
            .map(|source| {
                source
                    .xml_errors
                    .map(|source| source.map(|source| source.report))
            })
            .transpose()
            .map(Option::flatten)
    }
    /// The summary and its matching XML artifact share one database lock.
    /// A corrupt XML artifact does not erase the usable summary from an export.
    pub fn diagnostic_index(&self) -> Result<Option<crate::diagnostics::DiagnosticIndex>> {
        let mut db = self.db()?;
        let tx = db.transaction()?;
        let snapshot = read_summary(&tx)?.map(|source| {
            let xml_errors = read_xml_errors(&tx, &source);
            crate::diagnostics::DiagnosticIndex {
                summary: source.summary,
                summary_bytes: source.bytes,
                summary_modified_unix: source.modified_unix,
                xml_errors,
            }
        });
        tx.commit()?;
        Ok(snapshot)
    }
    pub fn index_summary(&self) -> Result<Option<IndexSummary>> {
        Ok(self
            .index_summary_with_source()?
            .map(|(summary, _)| summary))
    }
    pub fn index_summary_with_source(&self) -> Result<Option<(IndexSummary, Vec<u8>)>> {
        Ok(read_summary(&*self.db()?)?.map(|source| (source.summary, source.bytes)))
    }
}
fn read_summary(db: &Connection) -> Result<Option<SummarySource>> {
    let current = db
        .query_row("SELECT body FROM preferences WHERE key=?1", [KEY], |row| {
            row.get::<_, String>(0)
        })
        .optional()?;
    match current {
        Some(body) => Ok(Some(SummarySource {
            summary: serde_json::from_str(&body)?,
            bytes: body.into_bytes(),
            import: None,
            modified_unix: None,
        })),
        None => legacy_summary(db),
    }
}
fn read_xml_errors(
    db: &Connection,
    source: &SummarySource,
) -> Result<Option<crate::diagnostics::XmlErrorSource>> {
    let bytes = if let Some(import) = &source.import {
        let mut statement = db.prepare("SELECT path,sha256,body,modified_unix FROM legacy_artifacts WHERE import_id=?1 AND path IN ('manager-xml-errors.json','data/manager-xml-errors.json') ORDER BY path")?;
        let mut original: Option<(Vec<u8>, Option<i64>)> = None;
        for row in statement.query_map([import], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, Option<i64>>(3)?,
            ))
        })? {
            let (path, expected, bytes, modified_unix) = row?;
            if hash(&bytes) != expected {
                return Err(AppError::new(
                    "migration-archive-corrupt",
                    "Imported XML details failed their original hash check",
                )
                .at(path));
            }
            if original.as_ref().is_some_and(|(prior, _)| prior != &bytes) {
                return Err(AppError::new(
                    "migration-xml-ambiguous",
                    "Imported XML detail copies disagree; the original files are preserved",
                )
                .at(path));
            }
            original = Some((bytes, modified_unix));
        }
        original
    } else {
        db.query_row(
            "SELECT body FROM preferences WHERE key='manager-xml-errors'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(|body| (body.into_bytes(), None))
    };
    let Some((bytes, modified_unix)) = bytes else {
        if source
            .summary
            .scan_stats
            .as_ref()
            .is_some_and(|stats| stats.xml_read_errors > 0)
        {
            return Err(AppError::new(
                "diagnostics-xml-missing",
                "The saved summary reports XML failures but its matching detail file is missing",
            )
            .at("manager-xml-errors.json"));
        }
        return Ok(None);
    };
    let report: XmlErrorReport =
        serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes)).map_err(
            |error| AppError::new("diagnostics-xml-invalid", error).at("manager-xml-errors.json"),
        )?;
    let modified_unix = modified_unix.or_else(|| {
        chrono::DateTime::parse_from_rfc3339(&report.generated_at)
            .ok()
            .map(|time| time.timestamp())
    });
    let time_matches = if source.import.is_some() {
        // Original PowerShell captures these timestamps separately: XML details
        // are written after the summary's timestamp was sampled, not at equality.
        match (
            chrono::DateTime::parse_from_rfc3339(&source.summary.generated_at),
            chrono::DateTime::parse_from_rfc3339(&report.generated_at),
        ) {
            (Ok(summary), Ok(details)) => details >= summary,
            _ => false,
        }
    } else {
        report.generated_at == source.summary.generated_at
    };
    if !time_matches
        || report.count != report.errors.len() as u64
        || source
            .summary
            .scan_stats
            .as_ref()
            .is_some_and(|stats| stats.xml_read_errors != report.count)
    {
        return Err(AppError::new(
            "diagnostics-state",
            "XML error details do not match the saved scan summary",
        )
        .at("manager-xml-errors.json"));
    }
    Ok(Some(crate::diagnostics::XmlErrorSource {
        report,
        bytes,
        modified_unix,
    }))
}

fn legacy_summary(db: &Connection) -> Result<Option<SummarySource>> {
    // The original import identity binds product kind and reviewed source even
    // after the operation's plan has been replaced by its completed receipt.
    let mut candidates = db.prepare("SELECT DISTINCT a.import_id,o.fingerprint,o.result FROM legacy_artifacts a JOIN operations o ON o.id=a.import_id WHERE a.path IN ('manager-index-summary.json','data/manager-index-summary.json') AND json_extract(o.result,'$.kind')='migration' ORDER BY o.rowid DESC")?;
    let mut import = None;
    for row in candidates.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })? {
        let (id, fingerprint, body) = row?;
        let OperationResult::Migration(receipt) = serde_json::from_str(&body)? else {
            continue;
        };
        let mut original = false;
        for kind in ["tcm-portable", "tcm-state"] {
            original |= fingerprint
                == hash(&serde_json::to_vec(&(
                    "legacy-import",
                    kind,
                    &receipt.source,
                ))?);
        }
        if original {
            import = Some(id);
            break;
        }
    }
    let Some(import) = import else {
        return Ok(None);
    };
    let mut files = db.prepare("SELECT path,sha256,body,modified_unix FROM legacy_artifacts WHERE import_id=?1 AND path IN ('manager-index-summary.json','data/manager-index-summary.json') ORDER BY path")?;
    let mut previous = None;
    let mut summary = None;
    for row in files.query_map([&import], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Vec<u8>>(2)?,
            row.get::<_, Option<i64>>(3)?,
        ))
    })? {
        let (path, expected, bytes, modified_unix) = row?;
        if hash(&bytes) != expected {
            return Err(AppError::new(
                "migration-archive-corrupt",
                "Imported index summary failed its original hash check",
            )
            .at(path));
        }
        if previous.as_ref().is_some_and(|digest| digest != &expected) {
            return Err(AppError::new(
                "migration-summary-ambiguous",
                "Imported index summaries disagree; the original files are preserved",
            )
            .at(path));
        }
        summary = Some(SummarySource {
            summary: parse_legacy_summary(&bytes, &import).map_err(|e| e.at(&path))?,
            bytes,
            import: Some(import.clone()),
            modified_unix,
        });
        previous = Some(expected);
    }
    Ok(summary)
}
fn parse_legacy_summary(bytes: &[u8], import: &str) -> Result<IndexSummary> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))?;
    let invalid = || {
        AppError::new(
            "migration-summary-invalid",
            "Original Manager index summary has an unsupported structure",
        )
    };
    if value["version"] != 5 {
        return Err(invalid());
    }
    let items = value["items"].as_object().ok_or_else(invalid)?;
    let scan_stats = value
        .get("scanStats")
        .filter(|value| !value.is_null())
        .map(|value| serde_json::from_value::<ScanStats>(value.clone()))
        .transpose()?;
    let mut libraries = value
        .get("libraries")
        .filter(|value| !value.is_null())
        .map(|value| serde_json::from_value::<Vec<SummaryLibrary>>(value.clone()))
        .transpose()?
        .unwrap_or_default();
    let library_roots = value
        .get("libraryRoots")
        .filter(|value| !value.is_null())
        .map(|value| serde_json::from_value::<Vec<String>>(value.clone()))
        .transpose()?
        .unwrap_or_default();
    if library_roots.is_empty() {
        libraries.clear();
    }
    let generated_at = match value.get("generatedAt") {
        Some(serde_json::Value::String(value)) => value.clone(),
        None => String::new(),
        _ => return Err(invalid()),
    };
    Ok(IndexSummary {
        generated_at,
        source_task: format!("legacy-import:{import}"),
        indexed_titles: items.len() as u64,
        scan_stats,
        libraries,
        library_roots,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corrupt_archived_summary_is_rejected_without_changing_other_state() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let old = root.join("original");
        std::fs::create_dir(&old).unwrap();
        let bytes = br#"{"version":5,"items":{}}"#;
        std::fs::write(old.join("manager-index-summary.json"), bytes).unwrap();
        let store = Store::open(&root.join("state.sqlite")).unwrap();
        let plan = store
            .prepare_migration("import", &old, "tcm-state")
            .unwrap();
        store.apply_migration("import", &plan.fingerprint).unwrap();
        let config = store.configuration().unwrap();
        store
            .db()
            .unwrap()
            .execute(
                "UPDATE legacy_artifacts SET body=?1 WHERE import_id='import'",
                [b"corrupt".as_slice()],
            )
            .unwrap();
        assert_eq!(
            store.index_summary().unwrap_err().code,
            "migration-archive-corrupt"
        );
        assert_eq!(
            store.index_summary_with_source().unwrap_err().code,
            "migration-archive-corrupt"
        );
        assert_eq!(store.configuration().unwrap(), config);
        assert!(store.tasks().unwrap().is_empty());
        assert!(store.all_items().unwrap().is_empty());
        assert_eq!(
            std::fs::read(old.join("manager-index-summary.json")).unwrap(),
            bytes
        );
    }
    fn group_fixture() -> (tempfile::TempDir, Store, Configuration) {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let roots = [("movies", Space::Movie, "movie"), ("tv", Space::Tv, "tvshow")].into_iter().map(|(id, space, element)| {
            let path = base.join(id);
            std::fs::create_dir(&path).unwrap();
            std::fs::write(path.join("title.nfo"), format!("<{element}><title>{id}</title><uniqueid type=\"imdb\">tt123456{}</uniqueid><technicalspecs source=\"IMDb\"><section name=\"Camera\"><item>ARRI</item></section></technicalspecs></{element}>", if id == "movies" { "7" } else { "8" })).unwrap();
            LibraryRoot { id: id.into(), space, path: path.to_str().unwrap().into() }
        }).collect();
        let store = Store::open(&base.join("state.sqlite")).unwrap();
        let config = store
            .configure(
                "setup",
                Configuration {
                    roots,
                    ..Default::default()
                },
            )
            .unwrap();
        (temp, store, config)
    }
    #[test]
    fn service_cycle_commits_one_complete_summary_and_keeps_global_counts_unsummed() {
        let (_temp, store, config) = group_fixture();
        std::fs::write(Path::new(&config.roots[1].path).join("bad.nfo"), "<tvshow>").unwrap();
        let tasks = store.submit_service_cycle("session", 1, false).unwrap();
        assert_eq!(tasks.len(), 2);
        assert!(store.index_summary().unwrap().is_none());
        store.run_service_scan("session", || false, |_| {}).unwrap();
        assert!(store.index_summary().unwrap().is_none());
        assert!(store.catalog_summary().unwrap().generated_at.is_none());
        assert_eq!(
            store.begin_diagnostic_job("between").unwrap_err().code,
            "task-busy"
        );
        let last = store
            .run_service_scan("session", || false, |_| {})
            .unwrap()
            .unwrap();
        assert_eq!(last.state, TaskState::Failed);
        let summary = store.index_summary().unwrap().unwrap();
        let stats = summary.scan_stats.as_ref().unwrap();
        assert_eq!(stats.online_roots_scanned, 2);
        assert_eq!(stats.nfo_seen, 3);
        assert_eq!(stats.nfo_reparsed, 3);
        assert_eq!(stats.xml_read_errors, 1);
        assert_eq!(stats.technical_specs_found, 2);
        assert_eq!(stats.web_eligible_specs_found, 2);
        assert_eq!(summary.indexed_titles, 2);
        assert!(!tasks.iter().any(|task| task.id == summary.source_task));
        assert_eq!(
            store.catalog_summary().unwrap().generated_at.as_deref(),
            Some(summary.generated_at.as_str())
        );
        assert_eq!(store.preferences(PENDING).unwrap(), serde_json::json!({}));
        store.submit_service_cycle("session", 2, false).unwrap();
        store.run_service_scan("session", || false, |_| {}).unwrap();
        assert_eq!(store.index_summary().unwrap().unwrap(), summary);
        store.run_service_scan("session", || false, |_| {}).unwrap();
        let current = store.index_summary().unwrap().unwrap();
        assert_eq!(current.scan_stats.unwrap().nfo_reparsed, 1);
    }
    #[test]
    fn cancelled_cycle_and_reopen_keep_previous_observation_and_next_cycle_starts_fresh() {
        let (temp, store, _config) = group_fixture();
        store.submit_service_cycle("first", 1, false).unwrap();
        for _ in 0..2 {
            store.run_service_scan("first", || false, |_| {}).unwrap();
        }
        let previous = store.index_summary().unwrap().unwrap();
        store.submit_service_cycle("first", 2, false).unwrap();
        store.run_service_scan("first", || false, |_| {}).unwrap();
        let partial = store
            .run_service_scan("first", || true, |_| {})
            .unwrap()
            .unwrap();
        assert_eq!(partial.state, TaskState::Interrupted);
        assert_eq!(store.index_summary().unwrap().unwrap(), previous);
        drop(store);
        let store = Store::open(&temp.path().canonicalize().unwrap().join("state.sqlite")).unwrap();
        assert_eq!(store.index_summary().unwrap().unwrap(), previous);
        assert!(store
            .tasks()
            .unwrap()
            .iter()
            .all(|task| task.state.terminal()));
        store.submit_service_cycle("next", 1, true).unwrap();
        for _ in 0..2 {
            store.run_service_scan("next", || false, |_| {}).unwrap();
        }
        let summary = store.index_summary().unwrap().unwrap();
        assert_eq!(summary.scan_stats.as_ref().unwrap().nfo_seen, 2);
        assert_eq!(summary.scan_stats.unwrap().nfo_reparsed, 0);
        assert!(summary.source_task.starts_with("refresh-"));
    }
    #[test]
    fn automatic_children_and_summary_plan_are_one_transaction() {
        let (_temp, store, _) = group_fixture();
        store.db().unwrap().execute_batch("CREATE TRIGGER fail_tv BEFORE INSERT ON tasks WHEN json_extract(NEW.body,'$.space')='tv' BEGIN SELECT RAISE(ABORT,'second child failed'); END;").unwrap();
        assert!(store.submit_service_cycle("session", 1, false).is_err());
        assert!(store.tasks().unwrap().is_empty());
        assert_eq!(store.preferences(PENDING).unwrap(), serde_json::json!({}));
        assert!(store.index_summary().unwrap().is_none());
        store
            .db()
            .unwrap()
            .execute_batch("DROP TRIGGER fail_tv")
            .unwrap();
        assert_eq!(
            store
                .submit_service_cycle("session", 1, false)
                .unwrap()
                .len(),
            2
        );
    }
    #[test]
    fn rebuild_and_directory_actions_keep_their_original_group_identity() {
        let (_temp, store, config) = group_fixture();
        store
            .submit_rebuild("rebuild-all", config.revision, "service")
            .unwrap();
        store.run_rebuild("service", || false, |_| {}).unwrap();
        assert!(store.index_summary().unwrap().is_none());
        store.run_rebuild("service", || false, |_| {}).unwrap();
        let summary = store.index_summary().unwrap().unwrap();
        assert_eq!(summary.source_task, "rebuild-all");
        assert_eq!(summary.scan_stats.unwrap().nfo_seen, 2);
        let folder = store
            .folder_settings()
            .unwrap()
            .folders
            .into_iter()
            .find(|folder| folder.path == config.roots[0].path)
            .unwrap();
        store
            .submit_manager_scan(
                "only-folder",
                config.revision,
                "service",
                crate::folders::ManagerScanScope::Folder(folder.id),
            )
            .unwrap();
        store.run_service_scan("service", || false, |_| {}).unwrap();
        let summary = store.index_summary().unwrap().unwrap();
        assert_eq!(summary.source_task, "only-folder");
        assert_eq!(summary.scan_stats.as_ref().unwrap().nfo_seen, 1);
        assert_eq!(summary.scan_stats.unwrap().technical_specs_found, 2);
    }
    fn mixed_fixture() -> (tempfile::TempDir, Store, Configuration) {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let path = base.join("mixed");
        std::fs::create_dir(&path).unwrap();
        for (name, element, imdb) in [
            ("movie", "movie", "tt1234567"),
            ("show", "tvshow", "tt1234568"),
            ("episode", "episodedetails", "tt1234568"),
        ] {
            std::fs::write(path.join(format!("{name}.nfo")), format!("\u{feff}<{element}>\r\n<title>{name}</title><uniqueid type=\"imdb\">{imdb}</uniqueid><technicalspecs source=\"IMDb\"><section name=\"Camera\"><item>ARRI</item></section></technicalspecs></{element}>\r\n")).unwrap();
        }
        let store = Store::open(&base.join("state.sqlite")).unwrap();
        let config = store
            .configure(
                "setup",
                Configuration {
                    roots: [Space::Movie, Space::Tv]
                        .into_iter()
                        .map(|space| LibraryRoot {
                            id: if space == Space::Movie { "movie" } else { "tv" }.into(),
                            space,
                            path: path.to_str().unwrap().into(),
                        })
                        .collect(),
                    ..Default::default()
                },
            )
            .unwrap();
        (temp, store, config)
    }
    #[test]
    fn mixed_actions_visit_each_physical_root_once_and_route_both_media_spaces() {
        for mode in ["cycle", "rebuild", "folder"] {
            let (_temp, store, config) = mixed_fixture();
            let root = Path::new(&config.roots[0].path);
            let before: Vec<_> = std::fs::read_dir(root)
                .unwrap()
                .map(|entry| {
                    let path = entry.unwrap().path();
                    (
                        path.clone(),
                        std::fs::read(&path).unwrap(),
                        std::fs::metadata(path).unwrap().modified().unwrap(),
                    )
                })
                .collect();
            let tasks = match mode {
                "cycle" => store.submit_service_cycle("service", 1, false).unwrap(),
                "rebuild" => store
                    .submit_rebuild("rebuild", config.revision, "service")
                    .unwrap(),
                _ => {
                    let folder = store.folder_settings().unwrap().folders[0].id.clone();
                    store
                        .submit_manager_scan(
                            "folder",
                            config.revision,
                            "service",
                            crate::folders::ManagerScanScope::Folder(folder),
                        )
                        .unwrap()
                }
            };
            let first = if mode == "rebuild" {
                store.run_rebuild("service", || false, |_| {})
            } else {
                store.run_service_scan("service", || false, |_| {})
            }
            .unwrap()
            .unwrap();
            assert_eq!(
                first.processed, 3,
                "{mode}: one physical traversal must classify both spaces"
            );
            assert!(store.index_summary().unwrap().is_none());
            // Moving the source after the owning task proves the second child
            // does not visit the directory again or overwrite its online observation.
            let moved = root.with_file_name("offline-after-read");
            std::fs::rename(root, &moved).unwrap();
            let second = if mode == "rebuild" {
                store.run_rebuild("service", || false, |_| {})
            } else {
                store.run_service_scan("service", || false, |_| {})
            }
            .unwrap()
            .unwrap();
            assert_eq!(second.id, tasks[1].id);
            assert_eq!(second.state, TaskState::Completed);
            assert_eq!(second.processed, 0);
            let summary = store.index_summary().unwrap().unwrap();
            let stats = summary.scan_stats.unwrap();
            assert_eq!(
                (
                    stats.online_roots_scanned,
                    stats.nfo_seen,
                    stats.nfo_reparsed,
                    stats.xml_read_errors
                ),
                (1, 3, 3, 0)
            );
            assert_eq!(
                (
                    stats.technical_specs_found,
                    stats.web_eligible_specs_found,
                    stats.episode_specs_excluded_from_web
                ),
                (3, 2, 1)
            );
            assert!(summary.libraries[0].online);
            let items = store.all_items().unwrap();
            assert_eq!(
                items
                    .iter()
                    .filter(|item| item.space == Space::Movie && item.root_id == "movie")
                    .count(),
                1
            );
            assert_eq!(
                items
                    .iter()
                    .filter(|item| item.space == Space::Tv && item.root_id == "tv")
                    .count(),
                2
            );
            std::fs::rename(&moved, root).unwrap();
            for (path, bytes, modified) in &before {
                assert_eq!(std::fs::read(path).unwrap(), *bytes);
                assert_eq!(
                    std::fs::metadata(path).unwrap().modified().unwrap(),
                    *modified
                );
            }
            store.submit_service_cycle("service", 2, false).unwrap();
            for _ in 0..2 {
                store.run_service_scan("service", || false, |_| {}).unwrap();
            }
            assert_eq!(
                store
                    .index_summary()
                    .unwrap()
                    .unwrap()
                    .scan_stats
                    .unwrap()
                    .nfo_reparsed,
                0
            );
        }
    }
    #[test]
    fn mixed_xml_failures_retry_once_and_successful_scan_prunes_both_logical_roots() {
        let (_temp, store, config) = mixed_fixture();
        let root = Path::new(&config.roots[0].path);
        std::fs::write(root.join("bad.nfo"), "<movie>").unwrap();
        for cycle in 1..=2 {
            store.submit_service_cycle("service", cycle, false).unwrap();
            for _ in 0..2 {
                store.run_service_scan("service", || false, |_| {}).unwrap();
            }
            let stats = store.index_summary().unwrap().unwrap().scan_stats.unwrap();
            assert_eq!(stats.xml_read_errors, 1);
            let report = store.index_xml_errors().unwrap().unwrap();
            assert_eq!(report.count, 1);
            assert_eq!(
                report.errors[0].path,
                root.join("bad.nfo").to_str().unwrap()
            );
            assert_eq!(stats.nfo_seen, 4);
            assert_eq!(stats.nfo_reparsed, if cycle == 1 { 4 } else { 1 });
            assert_eq!(
                store
                    .all_items()
                    .unwrap()
                    .iter()
                    .filter(|item| item.error.is_some())
                    .count(),
                1
            );
        }
        std::fs::remove_file(root.join("bad.nfo")).unwrap();
        std::fs::remove_file(root.join("episode.nfo")).unwrap();
        std::fs::write(
            root.join("movie.nfo"),
            "<tvshow><title>Reclassified</title></tvshow>",
        )
        .unwrap();
        store.submit_service_cycle("service", 3, false).unwrap();
        for _ in 0..2 {
            store.run_service_scan("service", || false, |_| {}).unwrap();
        }
        let items = store.all_items().unwrap();
        assert_eq!(items.len(), 2);
        assert!(items
            .iter()
            .all(|item| item.error.is_none() && item.space == Space::Tv && item.root_id == "tv"));
        assert_eq!(
            store
                .index_summary()
                .unwrap()
                .unwrap()
                .scan_stats
                .unwrap()
                .nfo_reparsed,
            1
        );
    }
}
