use super::*;
impl Store {
    /// One explicit original "rebuild-index" action owns both media spaces.
    pub fn submit_rebuild(&self, id: &str, revision: u32, session: &str) -> Result<Vec<Task>> {
        valid_id(id)?;
        valid_id(session)?;
        let fingerprint = hash(&serde_json::to_vec(&("rebuild-index", revision, session))?);
        let mut db = self.db()?;
        self.writable()?;
        let tx = db.transaction()?;
        if let Some((old, body)) = tx
            .query_row(
                "SELECT fingerprint,result FROM operations WHERE id=?1",
                [id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
        {
            if old != fingerprint {
                return Err(AppError::new(
                    "operation-conflict",
                    "Rebuild operation has different input",
                ));
            }
            return match serde_json::from_str(&body)? {
                OperationResult::Rebuild(tasks) => Ok(tasks),
                _ => Err(AppError::new(
                    "operation-conflict",
                    "Operation belongs to another action",
                )),
            };
        }
        if tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id=?1)",
            [id],
            |row| row.get::<_, bool>(0),
        )? {
            return Err(AppError::new(
                "operation-conflict",
                "Operation belongs to a task",
            ));
        }
        jobs::require_no_diagnostic(&tx)?;
        let config = tx
            .query_row("SELECT body FROM configuration WHERE id=1", [], |row| {
                row.get::<_, String>(0)
            })
            .optional()?
            .map(|body| serde_json::from_str::<Configuration>(&body))
            .transpose()?
            .unwrap_or_default();
        if config.revision != revision {
            return Err(AppError::new(
                "configuration-conflict",
                "Settings changed before rebuilding",
            ));
        }
        if config.roots.is_empty() {
            return Err(AppError::new("empty-scope", "No enabled library roots"));
        }
        if tx.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE json_extract(body,'$.state') IN ('requested','running','paused','interrupted'))",[],|row|row.get::<_,bool>(0))? {return Err(AppError::new("task-busy","Wait for the current check to finish"));}
        // Original Assert-FullRebuildRootsOnline runs before any index work.
        for root in &config.roots {
            let checked = paths::checked(Path::new(&root.path))?;
            std::fs::read_dir(&checked)
                .map_err(|error| AppError::new("root-unreadable", error).at(&root.path))?;
        }
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
            let child = format!("rebuild-{}", hash(&serde_json::to_vec(&(id, &space))?));
            if tx.query_row("SELECT EXISTS(SELECT 1 FROM operations WHERE id=?1 UNION ALL SELECT 1 FROM tasks WHERE id=?1)",[&child],|row|row.get::<_,bool>(0))? {return Err(AppError::new("operation-conflict","Rebuild task identity already exists"));}
            let task = Task {
                force_parse: true,
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
                serde_json::to_string(&OperationResult::Rebuild(tasks.clone()))?
            ],
        )?;
        tx.commit()?;
        Ok(tasks)
    }
    pub(crate) fn has_rebuild_work(&self, session: &str) -> Result<bool> {
        self.db()?.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE json_extract(body,'$.service_session')=?1 AND json_extract(body,'$.force_parse')=1 AND json_extract(body,'$.state')='requested')",[session],|row|row.get(0)).map_err(Into::into)
    }
    pub(crate) fn run_rebuild(
        &self,
        session: &str,
        stop: impl Fn() -> bool,
        progress: impl Fn(&Task),
    ) -> Result<Option<Task>> {
        self.run_next_context(Some(session), true, stop, progress)
    }
}
