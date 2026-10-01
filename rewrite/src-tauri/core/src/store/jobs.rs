use super::*;
const KEY: &str = "manager-job";
pub(super) fn require_no_diagnostic(db: &Connection) -> Result<()> {
    if db.query_row("SELECT EXISTS(SELECT 1 FROM preferences WHERE key=?1 AND json_extract(body,'$.job.action')='diagnose' AND json_extract(body,'$.job.running')=1)", [KEY], |row| row.get::<_,bool>(0))? {
        return Err(AppError::new("task-busy", "已有任务正在运行"));
    }
    Ok(())
}

fn log_tail(log: &str) -> &str {
    let mut start = log.len().saturating_sub(40000);
    while !log.is_char_boundary(start) {
        start += 1;
    }
    &log[start..]
}

fn imported_idle_job(db: &Connection) -> Result<Option<serde_json::Value>> {
    // A historical log is the original idle display, never a new task's output.
    if db.query_row("SELECT EXISTS(SELECT 1 FROM tasks)", [], |row| {
        row.get::<_, bool>(0)
    })? {
        return Ok(None);
    }
    let import: Option<String> = db.query_row("SELECT import_id FROM legacy_artifacts WHERE category='history' AND path IN ('job.log','logs/job.log') ORDER BY rowid DESC LIMIT 1", [], |row| row.get(0)).optional()?;
    let Some(import) = import else {
        return Ok(None);
    };
    let mut query = db.prepare("SELECT path,sha256,body FROM legacy_artifacts WHERE import_id=?1 AND category='history' AND path IN ('job.log','logs/job.log') ORDER BY path")?;
    let mut original = None;
    for row in query.query_map([import], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Vec<u8>>(2)?,
        ))
    })? {
        let (path, digest, bytes) = row?;
        if hash(&bytes) != digest {
            return Err(AppError::new(
                "history-integrity",
                "Historical diagnostic log failed its original checksum",
            )
            .at(path));
        }
        let text = String::from_utf8(bytes).map_err(|error| {
            AppError::new(
                "history-encoding",
                format!("Historical bytes are preserved but cannot be displayed as UTF-8: {error}"),
            )
            .at(&path)
        })?;
        if original.as_ref().is_some_and(|previous| previous != &text) {
            return Err(AppError::new(
                "history-ambiguous",
                "Imported job logs disagree; the original files are preserved",
            )
            .at(path));
        }
        original = Some(text);
    }
    Ok(original.map(|log| serde_json::json!({"running":false,"action":"","message":"","exit_code":0,"blocks_exit":false,"needs_admin":false,"log":log_tail(&log)})))
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct SavedJob {
    id: String,
    task_boundary: i64,
    job: serde_json::Value,
    #[serde(default)]
    text: crate::job_text::TaskText,
}

impl Store {
    pub fn begin_diagnostic_job(&self, id: &str) -> Result<()> {
        let config = self.configuration()?;
        let text = crate::job_text::TaskText::capture(self, &config.locale)?;
        let mut db = self.db()?;
        self.writable()?;
        let tx = db.transaction()?;
        let active: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE json_extract(body,'$.state') IN ('requested','running','paused','interrupted'))", [], |r| r.get(0))?;
        let running: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM preferences WHERE key=?1 AND json_extract(body,'$.job.running')=1)", [KEY], |r| r.get(0))?;
        if active || running {
            return Err(AppError::new("task-busy", "已有任务正在运行"));
        }
        let boundary =
            tx.query_row("SELECT COALESCE(MAX(rowid),0) FROM tasks", [], |r| r.get(0))?;
        let started = crate::emby::timestamp();
        let log = format!(
            "Tech Card Manager {}\n{}：{}\n{}：{}\n\n",
            env!("CARGO_PKG_VERSION"),
            text.text("任务"),
            text.text("运行诊断"),
            text.text("开始时间"),
            started
        );
        let saved = SavedJob {
            id: id.into(),
            task_boundary: boundary,
            job: serde_json::json!({"action":"diagnose","running":true,"started_at":started,"ended_at":"","exit_code":0,"message":text.text("运行中"),"log":log,"language":config.locale,"blocks_exit":false,"needs_admin":false}),
            text,
        };
        tx.execute("INSERT INTO preferences(key,body) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET body=excluded.body", params![KEY,serde_json::to_string(&saved)?])?;
        tx.commit()?;
        Ok(())
    }
    pub fn finish_diagnostic_job(
        &self,
        id: &str,
        result: &Result<crate::diagnostics::IndexDiagnostic>,
    ) -> Result<()> {
        let mut db = self.db()?;
        self.writable()?;
        let tx = db.transaction()?;
        let body: String =
            tx.query_row("SELECT body FROM preferences WHERE key=?1", [KEY], |r| {
                r.get(0)
            })?;
        let mut saved: SavedJob = serde_json::from_str(&body)?;
        if saved.id != id || saved.job["action"] != "diagnose" || saved.job["running"] != true {
            return Err(AppError::new("job-conflict", "任务记录已改变"));
        }
        let text = &saved.text;
        let mut log = saved.job["log"].as_str().unwrap_or_default().to_owned();
        let (code, message) = match result {
            Ok(report) => {
                if let Some(summary) = &report.summary {
                    let count = summary.indexed_titles;
                    log.push_str(&format!(
                        "✅ {} {count} {}。\n",
                        text.text("当前技术规格索引包含"),
                        text.text("个 IMDb 条目")
                    ));
                    if let Some(stats) = &summary.scan_stats {
                        log.push_str(&format!("{}：roots={}, NFO={}, reparsed={}, tech={}, webEligible={}, episodeWebSkipped={}, xmlErrors={}\n",text.text("扫描统计"),stats.online_roots_scanned,stats.nfo_seen,stats.nfo_reparsed,stats.technical_specs_found,stats.web_eligible_specs_found,stats.episode_specs_excluded_from_web,stats.xml_read_errors));
                    }
                    if !summary.libraries.is_empty() || !summary.library_roots.is_empty() {
                        log.push_str(&format!("\n{}：\n", text.text("自动发现的 Emby 媒体库")));
                        for root in &summary.libraries {
                            let kind = &root.kind;
                            let state = if root.online { "online" } else { "offline" };
                            log.push_str(&format!("  [{kind}] {}  {state}\n", root.path));
                        }
                        if summary.libraries.is_empty() {
                            for root in &summary.library_roots {
                                log.push_str(&format!("  {root}\n"));
                            }
                        }
                    }
                    log.push_str(&format!("\n{}：\n", text.text("Web 前端")));
                    if report.frontend_ok {
                        log.push_str(&format!(
                            "  ✅ {}{} {}。\n",
                            text.text("dashboard-ui 注入与 v"),
                            env!("CARGO_PKG_VERSION"),
                            text.text("JS 均正常")
                        ));
                    } else {
                        log.push_str(&format!(
                            "  ❌ {}。\n",
                            text.text("dashboard-ui 注入或前端 JS 版本不正确")
                        ));
                    }
                    if summary
                        .scan_stats
                        .as_ref()
                        .is_some_and(|stats| stats.xml_read_errors > 0)
                    {
                        if let Some(path) = &report.xml_errors_path {
                            log.push_str(&format!("  ℹ️ {}：{path}\n", text.text("XML 错误明细")));
                        }
                    }
                } else {
                    log.push_str(&format!("❌ {}。\n", text.text("Manager 索引摘要尚未生成")));
                }
                // CheckOnly reports an absent index or an unhealthy frontend in
                // its log and exits normally. Completion means the check ran.
                (0, text.text("完成"))
            }
            Err(error) => {
                let message = match &error.path {
                    Some(path) => format!("{} · {path}", error.message),
                    None => error.message.clone(),
                };
                log.push_str(&format!("\n{}：{message}\n", text.text("错误")));
                (1, message)
            }
        };
        saved.job["running"] = false.into();
        saved.job["ended_at"] = crate::emby::timestamp().into();
        saved.job["exit_code"] = code.into();
        saved.job["message"] = message.into();
        saved.job["log"] = log.into();
        tx.execute(
            "UPDATE preferences SET body=?2 WHERE key=?1",
            params![KEY, serde_json::to_string(&saved)?],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn begin_discovery_job(&self, id: &str) -> Result<()> {
        let config = self.configuration()?;
        let text = crate::job_text::TaskText::capture(self, &config.locale)?;
        let mut db = self.db()?;
        self.writable()?;
        let tx = db.transaction()?;
        require_no_diagnostic(&tx)?;
        let boundary =
            tx.query_row("SELECT COALESCE(MAX(rowid),0) FROM tasks", [], |r| r.get(0))?;
        let started = crate::emby::timestamp();
        let log = text.text(&format!("Tech Card Manager {}\n任务：从 Emby 发现媒体目录\n开始时间：{}\n\n\n=== Emby 电影 / 电视剧物理路径自动发现测试（只读） ===\n原则：按 library.db 真实父子关系逐个物理根判断；绝不合并公共父目录。\n\n", env!("CARGO_PKG_VERSION"), started));
        let value = SavedJob {
            id: id.into(),
            task_boundary: boundary,
            job: serde_json::json!({"action":"discover-roots","running":true,"started_at":started,"ended_at":"","exit_code":0,"message":text.text("运行中"),"log":log,"language":config.locale,"blocks_exit":false,"needs_admin":false}),
            text,
        };
        tx.execute("INSERT INTO preferences(key,body) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET body=excluded.body", params![KEY,serde_json::to_string(&value)?])?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn finish_discovery_job(
        &self,
        id: &str,
        result: &Result<Vec<crate::emby_libraries::DiscoveredLibrary>>,
    ) -> Result<()> {
        let db = self.db()?;
        self.writable()?;
        let body: String =
            db.query_row("SELECT body FROM preferences WHERE key=?1", [KEY], |r| {
                r.get(0)
            })?;
        let mut value: SavedJob = serde_json::from_str(&body)?;
        if value.id != id {
            return Err(AppError::new("job-conflict", "任务记录已改变"));
        }
        let mut log = value.job["log"].as_str().unwrap_or_default().to_string();
        let text = &value.text;
        let (code, message) = match result {
            Ok(rows) => {
                let included: Vec<_> = rows.iter().filter(|row| !row.spaces.is_empty()).collect();
                if included.is_empty() {
                    log.push_str(&text.text("❌ 没有识别出电影/电视剧物理路径。\n"));
                } else {
                    log.push_str(&text.text(&format!(
                        "✅ 识别出 {} 个电影/电视剧物理路径：\n\n",
                        included.len()
                    )));
                }
                for row in included {
                    let kind = if row.spaces.len() > 1 {
                        "Mixed Movie/TV"
                    } else if row.spaces[0] == Space::Tv {
                        "TV"
                    } else {
                        "Movies"
                    };
                    log.push_str(&format!(
                        "[{kind}] {}\n  {}：{}  (Id={}, ParentId={})\n  {}：{}\n  {}：{}\n\n",
                        row.server_path,
                        text.text("Emby 节点"),
                        row.name,
                        row.id,
                        row.parent_id.as_deref().unwrap_or_default(),
                        text.text("状态"),
                        text.text(if row.local_path.is_some() {
                            "在线"
                        } else {
                            "离线/不可访问"
                        }),
                        text.text("证据"),
                        row.evidence
                    ));
                }
                let ignored: Vec<_> = rows.iter().filter(|row| row.spaces.is_empty()).collect();
                if !ignored.is_empty() {
                    log.push_str(&text.text("以下物理根已识别，但会排除：\n"));
                    for row in ignored {
                        log.push_str(&format!(
                            "  [Ignored] {}  ({})\n",
                            row.server_path, row.evidence
                        ));
                    }
                    log.push('\n');
                }
                log.push_str(&text.text("支持：多个电影库、多个电视剧库、同一虚拟库多个物理路径、电影/电视剧混合库。\n离线路径不会导致旧索引被清空；音乐库和普通视频库不会进入技术规格扫描。\n\n确认这里列出的路径正确后，再不带 -DiscoverOnly 运行同一文件正式安装。\n"));
                if rows.iter().all(|row| row.spaces.is_empty()) {
                    (2, text.text("没有识别出电影/电视剧物理路径。"))
                } else {
                    (0, text.text("完成"))
                }
            }
            Err(error) => {
                let message = match &error.path {
                    Some(path) => format!("{} · {path}", text.text(&error.message)),
                    None => text.text(&error.message),
                };
                log.push_str(&format!("\n{}：{message}\n", text.text("错误")));
                (1, message)
            }
        };
        value.job["running"] = false.into();
        value.job["ended_at"] = crate::emby::timestamp().into();
        value.job["exit_code"] = code.into();
        value.job["message"] = message.into();
        value.job["log"] = log.into();
        db.execute(
            "UPDATE preferences SET body=?2 WHERE key=?1",
            params![KEY, serde_json::to_string(&value)?],
        )?;
        Ok(())
    }
    pub fn manager_job(&self) -> Result<Option<serde_json::Value>> {
        let db = self.db()?;
        let body: Option<String> = db
            .query_row("SELECT body FROM preferences WHERE key=?1", [KEY], |r| {
                r.get(0)
            })
            .optional()?;
        let Some(body) = body else {
            return imported_idle_job(&db);
        };
        let mut value: SavedJob = serde_json::from_str(&body)?;
        let boundary: i64 =
            db.query_row("SELECT COALESCE(MAX(rowid),0) FROM tasks", [], |r| r.get(0))?;
        if boundary > value.task_boundary {
            return Ok(None);
        };
        if let Some(log) = value.job["log"].as_str() {
            value.job["log"] = log_tail(log).into();
        }
        Ok(Some(value.job))
    }
    pub fn manager_job_log(&self) -> Result<Option<String>> {
        let body = self.preferences(KEY)?;
        Ok(body
            .get("job")
            .and_then(|v| v.get("log"))
            .and_then(|v| v.as_str())
            .map(str::to_owned))
    }
    pub(crate) fn recover_manager_job(&self) -> Result<()> {
        let body = self.preferences(KEY)?;
        if body.get("job").is_some() {
            // The original process starts with an idle JobState and displays
            // the previous job.log verbatim. Do not resume, relabel as success,
            // or append invented output to an interrupted historical log.
            let mut saved: SavedJob = serde_json::from_value(body)?;
            saved.job["running"] = false.into();
            saved.job["action"] = "".into();
            saved.job["message"] = "".into();
            saved.job["exit_code"] = 0.into();
            saved.job["blocks_exit"] = false.into();
            self.save_preference(KEY, &serde_json::to_value(saved)?)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn diagnostic(indexed_titles: Option<u64>) -> crate::diagnostics::IndexDiagnostic {
        crate::diagnostics::IndexDiagnostic {
            summary: indexed_titles.map(|indexed_titles| crate::diagnostics::IndexSummary {
                indexed_titles,
                generated_at: "test".into(),
                source_task: "test".into(),
                scan_stats: Some(Default::default()),
                libraries: vec![],
                library_roots: vec![],
            }),
            frontend_ok: false,
            xml_errors_path: None,
        }
    }
    #[test]
    fn xml_detail_hint_uses_the_captured_language_and_preserves_the_real_path() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let store = Store::open(&root.join("state.sqlite")).unwrap();
        store
            .configure(
                "english",
                Configuration {
                    locale: Locale::English,
                    ..Default::default()
                },
            )
            .unwrap();
        store.begin_diagnostic_job("details").unwrap();
        let path = root
            .join("中文路径-manager-xml-errors.json")
            .to_str()
            .unwrap()
            .to_string();
        let mut report = diagnostic(Some(1));
        report
            .summary
            .as_mut()
            .unwrap()
            .scan_stats
            .as_mut()
            .unwrap()
            .xml_read_errors = 1;
        report.xml_errors_path = Some(path.clone());
        store.finish_diagnostic_job("details", &Ok(report)).unwrap();
        assert!(store
            .manager_job_log()
            .unwrap()
            .unwrap()
            .contains(&format!("  ℹ️ XML error details：{path}\n")));
        store.begin_diagnostic_job("no-errors").unwrap();
        let mut report = diagnostic(Some(1));
        report.xml_errors_path = Some(path);
        store
            .finish_diagnostic_job("no-errors", &Ok(report))
            .unwrap();
        assert!(!store
            .manager_job_log()
            .unwrap()
            .unwrap()
            .contains("XML error details"));
    }
    #[test]
    fn explicit_diagnostic_keeps_original_output_across_reopen_and_rejects_late_completion() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.sqlite");
        let store = Store::open(&path).unwrap();
        store.begin_diagnostic_job("first").unwrap();
        assert_eq!(
            store.begin_diagnostic_job("duplicate").unwrap_err().code,
            "task-busy"
        );
        assert_eq!(
            store.begin_discovery_job("discovery").unwrap_err().code,
            "task-busy"
        );
        assert!(store.has_manual_work().unwrap());
        store
            .finish_diagnostic_job("first", &Ok(diagnostic(None)))
            .unwrap();
        let job = store.manager_job().unwrap().unwrap();
        assert_eq!(job["action"], "diagnose");
        assert_eq!(job["exit_code"], 0);
        let log = store.manager_job_log().unwrap().unwrap();
        assert!(log.ends_with("❌ Manager 索引摘要尚未生成。\n"));
        assert!(!store.has_manual_work().unwrap());
        assert!(store
            .finish_diagnostic_job("first", &Ok(diagnostic(Some(99))))
            .is_err());
        drop(store);
        let restored = Store::open(&path).unwrap();
        assert_eq!(restored.manager_job_log().unwrap().unwrap(), log);
        assert_eq!(restored.manager_job().unwrap().unwrap()["action"], "");
        restored.begin_diagnostic_job("interrupted").unwrap();
        let partial = restored.manager_job_log().unwrap().unwrap();
        drop(restored);
        let restored = Store::open(&path).unwrap();
        assert_eq!(restored.manager_job_log().unwrap().unwrap(), partial);
        assert_eq!(restored.manager_job().unwrap().unwrap()["action"], "");
        assert!(restored
            .finish_diagnostic_job("interrupted", &Ok(diagnostic(Some(99))))
            .is_err());
    }
    #[test]
    fn diagnostic_captures_language_and_serializes_against_scan_submission() {
        let temp = tempfile::tempdir().unwrap();
        let media = temp.path().canonicalize().unwrap().join("完成");
        std::fs::create_dir(&media).unwrap();
        let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
        let config = store
            .configure(
                "config",
                Configuration {
                    locale: Locale::English,
                    roots: vec![LibraryRoot {
                        id: "root".into(),
                        space: Space::Movie,
                        path: media.to_string_lossy().into(),
                    }],
                    ..Default::default()
                },
            )
            .unwrap();
        let root = config.roots[0].clone();
        store.begin_diagnostic_job("inspect").unwrap();
        let scan = ScanRequest {
            operation_id: "scan".into(),
            space: Space::Movie,
            root_ids: vec![root.id.clone()],
        };
        assert_eq!(store.submit(scan.clone()).unwrap_err().code, "task-busy");
        assert_eq!(
            store
                .submit_rebuild("rebuild", config.revision, "session")
                .unwrap_err()
                .code,
            "task-busy"
        );
        assert_eq!(
            store
                .submit_manager_scan(
                    "manager",
                    config.revision,
                    "session",
                    crate::folders::ManagerScanScope::Space(Space::Movie)
                )
                .unwrap_err()
                .code,
            "task-busy"
        );
        let mut changed = config;
        changed.locale = Locale::Simplified;
        store.configure("locale", changed).unwrap();
        let mut output = diagnostic(Some(3));
        output.frontend_ok = true;
        output
            .summary
            .as_mut()
            .unwrap()
            .libraries
            .push(crate::diagnostics::SummaryLibrary {
                path: root.path.clone(),
                kind: "Movies".into(),
                online: true,
            });
        store.finish_diagnostic_job("inspect", &Ok(output)).unwrap();
        let log = store.manager_job_log().unwrap().unwrap();
        assert!(log.contains("The current Technical Specs index contains 3 IMDb items"));
        assert!(log.contains(&root.path));
        assert!(log.contains("dashboard-ui injection and v5.0.0 JS are both healthy"));
        assert_eq!(
            store.manager_job().unwrap().unwrap()["message"],
            "Completed"
        );
        store.submit(scan).unwrap();
        assert_eq!(
            store.begin_diagnostic_job("during-scan").unwrap_err().code,
            "task-busy"
        );
    }
    #[test]
    fn diagnostic_read_failure_is_persisted_without_turning_into_success() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
        store.begin_diagnostic_job("failed").unwrap();
        store
            .finish_diagnostic_job(
                "failed",
                &Err(AppError::new("fixture-read", "permission denied").at("/电影/完成")),
            )
            .unwrap();
        let job = store.manager_job().unwrap().unwrap();
        assert_eq!(job["running"], false);
        assert_eq!(job["exit_code"], 1);
        assert!(store
            .manager_job_log()
            .unwrap()
            .unwrap()
            .contains("permission denied · /电影/完成"));
    }
    #[test]
    fn reopen_restores_original_idle_state_and_keeps_old_output_verbatim() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("manager.sqlite");
        let store = Store::open(&path).unwrap();
        store
            .configure(
                "language",
                Configuration {
                    locale: Locale::English,
                    ..Default::default()
                },
            )
            .unwrap();
        store.begin_discovery_job("first").unwrap();
        let before = store.manager_job_log().unwrap().unwrap();
        drop(store);
        let store = Store::open(&path).unwrap();
        let recovered = store.manager_job().unwrap().unwrap();
        assert_eq!(recovered["running"], false);
        assert_eq!(recovered["action"], "");
        assert_eq!(recovered["language"], "en-US");
        assert_eq!(store.manager_job_log().unwrap().unwrap(), before);
        let mut config = store.configuration().unwrap();
        config.locale = Locale::Simplified;
        store.configure("switch", config).unwrap();
        assert_eq!(store.manager_job().unwrap().unwrap(), recovered);
        store.begin_discovery_job("second").unwrap();
        assert_eq!(
            store
                .finish_discovery_job("first", &Ok(vec![]))
                .unwrap_err()
                .code,
            "job-conflict"
        );
        store.finish_discovery_job("second", &Ok(vec![])).unwrap();
        assert_eq!(store.manager_job().unwrap().unwrap()["exit_code"], 2);
    }
    #[test]
    fn visible_tail_preserves_unicode_and_export_retains_the_full_log() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("manager.sqlite")).unwrap();
        store.begin_discovery_job("long").unwrap();
        let message = "电影\n".repeat(10000);
        store
            .finish_discovery_job("long", &Err(AppError::new("fixture", &message)))
            .unwrap();
        let full = store.manager_job_log().unwrap().unwrap();
        let visible = store.manager_job().unwrap().unwrap()["log"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(full.len() > 40000);
        assert!(visible.len() <= 40000);
        assert!(full.ends_with(&visible));
        assert!(visible.contains("电影"));
    }
    #[test]
    fn discovery_uses_start_language_and_never_translates_media_names_or_paths() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
        store
            .configure(
                "english",
                Configuration {
                    locale: Locale::English,
                    ..Default::default()
                },
            )
            .unwrap();
        store.begin_discovery_job("frozen").unwrap();
        let mut config = store.configuration().unwrap();
        config.locale = Locale::Simplified;
        store.configure("change", config).unwrap();
        let row=serde_json::from_value(serde_json::json!({"id":"20","parent_id":"2","name":"状态与完成","server_path":"/电影/完成","local_path":null,"spaces":["tv"],"movie_evidence":0,"series_evidence":1,"episode_evidence":0,"evidence":"Series=1","state":"offline-or-needs-mapping","issues":[]})).unwrap();
        store
            .finish_discovery_job("frozen", &Ok(vec![row]))
            .unwrap();
        let job = store.manager_job().unwrap().unwrap();
        assert_eq!(job["message"], "Completed");
        assert_eq!(job["language"], "en-US");
        let log = store.manager_job_log().unwrap().unwrap();
        assert!(log.contains("[TV] /电影/完成"));
        assert!(log.contains("Emby node：状态与完成"));
        assert!(log.contains("Evidence：Series=1"));
    }
}
