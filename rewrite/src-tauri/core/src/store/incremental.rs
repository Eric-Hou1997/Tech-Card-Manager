use super::*;
use crate::incremental::IncrementalSettings;
const SETTINGS: &str = "incremental-settings";
impl Store {
    pub(crate) fn submit_service_cycle(
        &self,
        session: &str,
        cycle: u32,
        explicit: bool,
    ) -> Result<Vec<Task>> {
        valid_id(session)?;
        let mut db = self.db()?;
        self.writable()?;
        let tx = db.transaction()?;
        jobs::require_no_diagnostic(&tx)?;
        if tx.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE json_extract(body,'$.state') IN ('requested','running','paused','interrupted'))", [], |row| row.get::<_, bool>(0))? {
            return Err(AppError::new("task-busy", "Wait for the current check to finish"));
        }
        let config = Self::folder_configuration(&tx)?;
        let prefix = if explicit { "refresh" } else { "incremental" };
        let group_id = format!("{prefix}-{}", hash(&serde_json::to_vec(&(session, cycle))?));
        let mut tasks = Vec::new();
        for space in [Space::Movie, Space::Tv] {
            let roots: Vec<_> = config
                .roots
                .iter()
                .filter(|root| root.space == space)
                .cloned()
                .collect();
            if roots.is_empty() {
                continue;
            }
            let id = format!(
                "{prefix}-{}",
                hash(&serde_json::to_vec(&(session, cycle, &space))?)
            );
            if tx.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE id=?1 UNION ALL SELECT 1 FROM operations WHERE id=?1)", [&id], |row| row.get::<_, bool>(0))? {
                return Err(AppError::new("operation-conflict", "Service cycle identity already exists"));
            }
            let fingerprint = hash(&serde_json::to_vec(&(
                "service-cycle",
                session,
                cycle,
                explicit,
                &config,
            ))?);
            let task = Task {
                force_parse: false,
                service_session: Some(session.into()),
                id,
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
        index_summary::begin_group(&tx, &group_id, &tasks, &config)?;
        tx.commit()?;
        Ok(tasks)
    }
    /// The old feasibility UI queued independent scans. The original Manager
    /// never resumes a scan with its service stopped; retain these as history.
    pub fn retire_unowned_scans(&self) -> Result<()> {
        let db = self.db()?;
        self.writable()?;
        db.execute("UPDATE tasks SET body=json_set(body,'$.state','cancelled','$.failure',json(?1)) WHERE json_extract(body,'$.service_session') IS NULL AND json_extract(body,'$.state') IN ('requested','running','paused','interrupted')",[serde_json::to_string(&AppError::new("service-stopped","服务已停止，旧扫描任务未恢复"))?])?;
        Ok(())
    }
    pub fn incremental_settings(&self) -> Result<IncrementalSettings> {
        let value = self.preferences(SETTINGS)?;
        let settings = if value == serde_json::json!({}) || value.is_null() {
            IncrementalSettings::default()
        } else {
            serde_json::from_value(value)?
        };
        settings.validate()?;
        Ok(settings)
    }
    pub fn save_incremental_settings(
        &self,
        id: &str,
        mut value: IncrementalSettings,
    ) -> Result<IncrementalSettings> {
        valid_id(id)?;
        value.validate()?;
        let fingerprint = hash(&serde_json::to_vec(&("incremental-settings", &value))?);
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
                    "Incremental operation has different input",
                ));
            }
            return match serde_json::from_str(&body)? {
                OperationResult::IncrementalSettings(value) => Ok(value),
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
        let prior = tx
            .query_row(
                "SELECT body FROM preferences WHERE key=?1",
                [SETTINGS],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .map(|s| serde_json::from_str::<IncrementalSettings>(&s))
            .transpose()?
            .unwrap_or_default();
        if prior.revision != value.revision {
            return Err(AppError::new(
                "incremental-settings-conflict",
                "Reload incremental settings before saving",
            ));
        }
        value.revision = value.revision.checked_add(1).ok_or_else(|| {
            AppError::new(
                "revision-overflow",
                "Incremental settings revision exhausted",
            )
        })?;
        tx.execute("INSERT INTO preferences VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET body=excluded.body",params![SETTINGS,serde_json::to_string(&value)?])?;
        tx.execute(
            "INSERT INTO operations VALUES(?1,?2,?3)",
            params![
                id,
                fingerprint,
                serde_json::to_string(&OperationResult::IncrementalSettings(value.clone()))?
            ],
        )?;
        tx.commit()?;
        Ok(value)
    }
    pub(crate) fn has_manual_work(&self) -> Result<bool> {
        self.db()?.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE json_extract(body,'$.service_session') IS NULL AND json_extract(body,'$.state') IN ('requested','running','paused','interrupted')) OR EXISTS(SELECT 1 FROM preferences WHERE key='manager-job' AND json_extract(body,'$.job.action')='diagnose' AND json_extract(body,'$.job.running')=1)",[],|r|r.get(0)).map_err(Into::into)
    }
    pub(crate) fn has_active_checks(&self) -> Result<bool> {
        self.db()?.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE json_extract(body,'$.state') IN ('requested','running','paused','interrupted')) OR EXISTS(SELECT 1 FROM preferences WHERE key='manager-job' AND json_extract(body,'$.job.action')='diagnose' AND json_extract(body,'$.job.running')=1)", [], |row| row.get(0)).map_err(Into::into)
    }
    pub(crate) fn cancel_service_scans(&self, session: &str) -> Result<()> {
        let db = self.db()?;
        if !db.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE json_extract(body,'$.service_session')=?1 AND json_extract(body,'$.state') IN ('requested','running','paused','interrupted'))", [session], |row| row.get::<_, bool>(0))? {
            return Ok(());
        }
        self.writable()?;
        db.execute("UPDATE tasks SET body=json_set(body,'$.state','cancelled') WHERE json_extract(body,'$.service_session')=?1 AND json_extract(body,'$.state') IN ('requested','running','paused','interrupted')",[session])?;
        Ok(())
    }
}
