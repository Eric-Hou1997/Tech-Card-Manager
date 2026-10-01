use super::*;
use crate::folders::*;
const KEY: &str = "media-folders";
impl Store {
    pub(crate) fn submit_manager_scan(
        &self,
        id: &str,
        revision: u32,
        session: &str,
        scope: ManagerScanScope,
    ) -> Result<Vec<Task>> {
        valid_id(id)?;
        valid_id(session)?;
        let fingerprint = hash(&serde_json::to_vec(&(
            "manager-scan",
            revision,
            session,
            &scope,
        ))?);
        let mut db = self.db()?;
        self.writable()?;
        let tx = db.transaction()?;
        if let Some((old, body)) = tx
            .query_row(
                "SELECT fingerprint,result FROM operations WHERE id=?1",
                [id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
        {
            if old != fingerprint {
                return Err(AppError::new("operation-conflict", "扫描操作参数已改变"));
            }
            return match serde_json::from_str(&body)? {
                OperationResult::ManagerScan(tasks) => Ok(tasks),
                _ => Err(AppError::new(
                    "operation-conflict",
                    "操作编号已用于其他动作",
                )),
            };
        }
        if tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id=?1)",
            [id],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(AppError::new(
                "operation-conflict",
                "操作编号已用于其他任务",
            ));
        }
        jobs::require_no_diagnostic(&tx)?;
        let config = Self::folder_configuration(&tx)?;
        if config.revision != revision {
            return Err(AppError::new(
                "configuration-conflict",
                "媒体目录设置已改变，请重新读取后再刷新",
            ));
        }
        let previous = tx
            .query_row("SELECT body FROM preferences WHERE key=?1", [KEY], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
            .map(|s| serde_json::from_str(&s))
            .transpose()?;
        let folders = from_configuration(&config, previous).folders;
        let (prefix, roots) = match scope {
            ManagerScanScope::Space(space) => {
                let paths: Vec<_> = folders
                    .iter()
                    .filter(|f| {
                        f.enabled
                            && matches!(
                                (&f.kind, &space),
                                (FolderKind::Movies, Space::Movie) | (FolderKind::Tv, Space::Tv)
                            )
                    })
                    .map(|f| &f.path)
                    .collect();
                let roots: Vec<_> = config
                    .roots
                    .iter()
                    .filter(|r| r.space == space && paths.contains(&&r.path))
                    .cloned()
                    .collect();
                if roots.is_empty() {
                    return Err(AppError::new(
                        "empty-scope",
                        if folders
                            .iter()
                            .any(|f| f.enabled && f.kind == FolderKind::Mixed)
                        {
                            "当前媒体库没有独立分类目录；请先在设置中把混合目录拆分或标记为电影/电视剧，已避免误扫另一类媒体"
                        } else {
                            "当前媒体库没有已启用且已分类的目录"
                        },
                    ));
                }
                ("scan-space", roots)
            }
            ManagerScanScope::Folder(id) => {
                let folder = folders
                    .iter()
                    .find(|f| f.id == id && f.enabled)
                    .ok_or_else(|| AppError::new("invalid-scope", "请先保存并启用媒体目录"))?;
                (
                    "scan-root",
                    config
                        .roots
                        .iter()
                        .filter(|r| r.path == folder.path)
                        .cloned()
                        .collect(),
                )
            }
        };
        if roots.is_empty() {
            return Err(AppError::new("empty-scope", "请先保存媒体目录"));
        }
        if tx.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE json_extract(body,'$.state') IN ('requested','running','paused','interrupted'))",[],|r|r.get::<_,bool>(0))? {return Err(AppError::new("task-busy","已有任务正在运行"));}
        let mut tasks = Vec::new();
        for space in [Space::Movie, Space::Tv] {
            let roots: Vec<_> = roots.iter().filter(|r| r.space == space).cloned().collect();
            if roots.is_empty() {
                continue;
            }
            let child = format!("{prefix}-{}", hash(&serde_json::to_vec(&(id, &space))?));
            if tx.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE id=?1 UNION ALL SELECT 1 FROM operations WHERE id=?1)",[&child],|r|r.get::<_,bool>(0))? {return Err(AppError::new("operation-conflict","扫描任务编号已存在"));}
            let task = Task {
                force_parse: false,
                service_session: Some(session.into()),
                id: child,
                state: TaskState::Requested,
                locale: config.locale.clone(),
                space,
                roots,
                attempt: 0,
                processed: 0,
                errors: 0,
                current_path: None,
                failure: None,
            };
            tx.execute(
                "INSERT INTO tasks VALUES(?1,?2,?3)",
                params![task.id, fingerprint, serde_json::to_string(&task)?],
            )?;
            tasks.push(task);
        }
        index_summary::begin_group(&tx, id, &tasks, &config)?;
        tx.execute(
            "INSERT INTO operations VALUES(?1,?2,?3)",
            params![
                id,
                fingerprint,
                serde_json::to_string(&OperationResult::ManagerScan(tasks.clone()))?
            ],
        )?;
        tx.commit()?;
        Ok(tasks)
    }
    pub(crate) fn has_manager_scan(&self, session: &str) -> Result<bool> {
        self.db()?.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE json_extract(body,'$.service_session')=?1 AND json_extract(body,'$.state')='requested' AND (id LIKE 'scan-space-%' OR id LIKE 'scan-root-%'))",[session],|r|r.get(0)).map_err(Into::into)
    }
    pub fn folder_settings(&self) -> Result<FolderSettings> {
        let mut db = self.db()?;
        let tx = db.transaction()?;
        let current = Self::folder_configuration(&tx)?;
        let previous: Option<FolderSettings> = tx
            .query_row("SELECT body FROM preferences WHERE key=?1", [KEY], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
            .map(|v| serde_json::from_str(&v))
            .transpose()?;
        Ok(from_configuration(&current, previous))
    }
    pub(super) fn folder_configuration(tx: &rusqlite::Connection) -> Result<Configuration> {
        Ok(tx
            .query_row("SELECT body FROM configuration WHERE id=1", [], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
            .map(|v| serde_json::from_str(&v))
            .transpose()?
            .unwrap_or_default())
    }
    pub fn save_folder_settings(
        &self,
        id: &str,
        mut settings: FolderSettings,
    ) -> Result<FolderReceipt> {
        valid_id(id)?;
        let fingerprint = hash(&serde_json::to_vec(&("media-folders", &settings))?);
        let mut db = self.db()?;
        self.writable()?;
        let tx = db.transaction()?;
        if let Some((old, body)) = tx
            .query_row(
                "SELECT fingerprint,result FROM operations WHERE id=?1",
                [id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
        {
            if old != fingerprint {
                return Err(AppError::new(
                    "operation-conflict",
                    "Folder operation has different input",
                ));
            }
            return match serde_json::from_str(&body)? {
                OperationResult::Folders(receipt) => Ok(receipt),
                _ => Err(AppError::new(
                    "operation-conflict",
                    "Operation belongs to another action",
                )),
            };
        }
        if tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id=?1)",
            [id],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(AppError::new(
                "operation-conflict",
                "Operation belongs to a task",
            ));
        }
        let current = Self::folder_configuration(&tx)?;
        if current.revision != settings.revision {
            return Err(AppError::new(
                "configuration-conflict",
                "Reload folders before saving",
            ));
        }
        if settings.folders.len() > 256 {
            return Err(AppError::new("invalid-folders", "媒体目录不能超过 256 个"));
        }
        if !settings.folders.iter().any(|f| f.enabled) {
            return Err(AppError::new(
                "invalid-folders",
                "Enable at least one media folder",
            ));
        }
        let mut configuration = Configuration {
            revision: current.revision,
            locale: current.locale.clone(),
            roots: vec![],
        };
        let mut ids = std::collections::BTreeSet::new();
        let mut paths_seen = std::collections::BTreeSet::new();
        for folder in &mut settings.folders {
            valid_id(&folder.id)?;
            if !ids.insert(folder.id.clone())
                || folder.name.len() > 1024
                || folder.path.len() > 32768
                || folder.name.chars().any(char::is_control)
                || folder.path.chars().any(char::is_control)
                || !Path::new(&folder.path).is_absolute()
            {
                return Err(AppError::new(
                    "invalid-folders",
                    "Invalid or duplicate media folder",
                ));
            }
            let real = paths::configured_directory(Path::new(&folder.path))?;
            folder.path = real
                .to_str()
                .ok_or_else(|| AppError::new("invalid-encoding", "Root path must be Unicode"))?
                .into();
            if folder.enabled {
                for root in &configuration.roots {
                    if Path::new(&root.path).starts_with(&folder.path)
                        || Path::new(&folder.path).starts_with(&root.path)
                    {
                        return Err(AppError::new(
                            "overlapping-roots",
                            "Media folders must not overlap",
                        ));
                    }
                }
                for space in folder.kind.spaces() {
                    let old = current
                        .roots
                        .iter()
                        .find(|r| r.path == folder.path && r.space == space);
                    configuration.roots.push(LibraryRoot {
                        id: old
                            .map(|r| r.id.clone())
                            .unwrap_or_else(|| hash(format!("{}:{space:?}", folder.id).as_bytes())),
                        space,
                        path: folder.path.clone(),
                    });
                }
            }
            let key = if cfg!(windows) {
                folder.path.to_lowercase()
            } else {
                folder.path.clone()
            };
            if !paths_seen.insert(key) {
                return Err(AppError::new(
                    "duplicate-root",
                    "This folder is already listed",
                ));
            }
        }
        let configuration = Self::write_configuration(&tx, &current, configuration)?;
        settings.revision = configuration.revision;
        let receipt = FolderReceipt {
            settings,
            configuration,
        };
        tx.execute("INSERT INTO preferences VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET body=excluded.body",params![KEY,serde_json::to_string(&receipt.settings)?])?;
        tx.execute(
            "INSERT INTO operations VALUES(?1,?2,?3)",
            params![
                id,
                fingerprint,
                serde_json::to_string(&OperationResult::Folders(receipt.clone()))?
            ],
        )?;
        tx.commit()?;
        Ok(receipt)
    }
}
