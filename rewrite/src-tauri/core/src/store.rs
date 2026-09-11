use crate::{contracts::*, hash, library, paths};
use rusqlite::{params, Connection, OptionalExtension};
use std::{
    path::Path,
    sync::{Mutex, MutexGuard},
};

pub struct Store {
    connection: Mutex<Connection>,
    worker: Mutex<()>,
    _owner: DatabaseLease,
    maintenance: std::sync::atomic::AtomicBool,
}
// Explicit unlock prevents an unrelated concurrently spawned child from
// retaining a fork-inherited lease until its exec closes inherited descriptors.
struct DatabaseLease(std::fs::File);
impl Drop for DatabaseLease {
    fn drop(&mut self) {
        if let Err(error) = self.0.unlock() {
            eprintln!("database-unlock: {error}");
        }
    }
}
fn valid_id(id: &str) -> Result<()> {
    if id.is_empty() || id.len() > 96 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(AppError::new(
            "invalid-operation-id",
            "Use a unique alphanumeric operation ID",
        ));
    }
    Ok(())
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let owner = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.with_extension("lock"))
            .map_err(|e| AppError::new("database-lock", e))?;
        owner
            .try_lock()
            .map_err(|e| AppError::new("database-in-use", e))?;
        let connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > 3 {
            return Err(AppError::new(
                "newer-database",
                "Database belongs to a newer application; refusing downgrade",
            ));
        }
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        connection.execute_batch("BEGIN IMMEDIATE;
          CREATE TABLE IF NOT EXISTS configuration (id INTEGER PRIMARY KEY CHECK(id=1), body TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS operations (id TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, result TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS tasks (id TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, body TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS items (id TEXT PRIMARY KEY, root_id TEXT NOT NULL, seen_task TEXT NOT NULL, body TEXT NOT NULL);
          CREATE INDEX IF NOT EXISTS items_root ON items(root_id);
          CREATE TABLE IF NOT EXISTS legacy_artifacts(import_id TEXT NOT NULL, path TEXT NOT NULL, category TEXT NOT NULL, sha256 TEXT NOT NULL, body BLOB NOT NULL, PRIMARY KEY(import_id,path));
          CREATE TABLE IF NOT EXISTS preferences(key TEXT PRIMARY KEY, body TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS catalog_revision(id INTEGER PRIMARY KEY CHECK(id=1), value INTEGER NOT NULL CHECK(value>=0 AND typeof(value)='integer'));
          INSERT OR IGNORE INTO catalog_revision VALUES(1,0);
          CREATE TRIGGER IF NOT EXISTS catalog_insert AFTER INSERT ON items BEGIN UPDATE catalog_revision SET value=value+1 WHERE id=1; END;
          CREATE TRIGGER IF NOT EXISTS catalog_update AFTER UPDATE OF body ON items WHEN OLD.body<>NEW.body BEGIN UPDATE catalog_revision SET value=value+1 WHERE id=1; END;
          CREATE TRIGGER IF NOT EXISTS catalog_delete AFTER DELETE ON items BEGIN UPDATE catalog_revision SET value=value+1 WHERE id=1; END;
          CREATE INDEX IF NOT EXISTS tasks_state ON tasks(json_extract(body,'$.state'));
          PRAGMA user_version=3; COMMIT;")?;
        let store = Self {
            connection: Mutex::new(connection),
            worker: Mutex::new(()),
            _owner: DatabaseLease(owner),
            maintenance: std::sync::atomic::AtomicBool::new(false),
        };
        for mut task in store.tasks()? {
            if matches!(task.state, TaskState::Running) {
                task.state = TaskState::Interrupted;
                store.save_task(&task)?;
            }
        }
        Ok(store)
    }
    fn db(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| AppError::new("state-unavailable", "State owner failed"))
    }
    pub fn prepare_migration(
        &self,
        id: &str,
        source: &Path,
        kind: &str,
    ) -> Result<crate::migration::MigrationPlan> {
        valid_id(id)?;
        let source = paths::checked(source)?;
        let identity = hash(
            serde_json::to_vec(&("legacy-import", kind, source.to_string_lossy()))?.as_slice(),
        );
        if let Some(old) = self.operation(id, &identity)? {
            return match serde_json::from_str::<OperationResult>(&old)? {
                OperationResult::MigrationPlan(plan) => Ok(plan),
                OperationResult::Migration(_) => Err(AppError::new(
                    "migration-already-imported",
                    "Query this operation for its completed receipt",
                )),
                _ => Err(AppError::new(
                    "operation-conflict",
                    "ID belongs to another operation",
                )),
            };
        }
        let plan = crate::migration::prepare(id, &source, kind, &self.configuration()?)?;
        let db = self.db()?;
        self.writable()?;
        if db.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id=?1)",
            [id],
            |row| row.get::<_, bool>(0),
        )? {
            return Err(AppError::new("operation-conflict", "ID belongs to a task"));
        }
        let result = serde_json::to_string(&OperationResult::MigrationPlan(plan.clone()))?;
        db.execute(
            "INSERT INTO operations(id,fingerprint,result) VALUES(?1,?2,?3)",
            params![id, identity, result],
        )?;
        Ok(plan)
    }
    pub fn apply_migration(
        &self,
        id: &str,
        fingerprint: &str,
    ) -> Result<crate::migration::MigrationReceipt> {
        let mut db = self.db()?;
        self.writable()?;
        let tx = db.transaction()?;
        let body: String =
            tx.query_row("SELECT result FROM operations WHERE id=?1", [id], |row| {
                row.get(0)
            })?;
        let plan = match serde_json::from_str::<OperationResult>(&body)? {
            OperationResult::Migration(receipt) if receipt.fingerprint == fingerprint => {
                return Ok(receipt)
            }
            OperationResult::MigrationPlan(plan) if plan.fingerprint == fingerprint => plan,
            _ => {
                return Err(AppError::new(
                    "migration-plan-mismatch",
                    "Reviewed migration does not match this operation",
                ))
            }
        };
        let current: Configuration = tx
            .query_row("SELECT body FROM configuration WHERE id=1", [], |row| {
                row.get::<_, String>(0)
            })
            .optional()?
            .map(|s| serde_json::from_str(&s))
            .transpose()?
            .unwrap_or_default();
        {
            let mut statement = tx.prepare("SELECT body FROM tasks")?;
            for body in statement.query_map([], |row| row.get::<_, String>(0))? {
                let task: Task = serde_json::from_str(&body?)?;
                if !task.state.terminal() {
                    return Err(AppError::new(
                        "active-task",
                        "Finish or cancel tasks before importing legacy data",
                    ));
                }
            }
        }
        let (configuration, pending_roots) =
            crate::migration::merged_configuration(&plan, &current)?;
        let mut preferences = std::collections::BTreeMap::new();
        let mut total = 0u64;
        for file in &plan.files {
            let data = crate::migration::read_snapshot(Path::new(&plan.source), file)?;
            if file.category == "configuration" {
                if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&data) {
                    preferences.insert(file.relative.clone(), value);
                }
            }
            tx.execute("INSERT INTO legacy_artifacts(import_id,path,category,sha256,body) VALUES(?1,?2,?3,?4,?5)",params![id,file.relative,file.category,file.hash,data])?;
            total = total
                .checked_add(file.bytes)
                .ok_or_else(|| AppError::new("migration-size", "Import byte count overflow"))?;
        }
        let preference_key = format!("legacy:{}", plan.source_kind);
        tx.execute("INSERT INTO preferences(key,body) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET body=excluded.body",params![preference_key,serde_json::to_string(&crate::migration::normalized_preferences(&preferences))?])?;
        tx.execute("INSERT INTO configuration(id,body) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET body=excluded.body",[serde_json::to_string(&configuration)?])?;
        let receipt = crate::migration::MigrationReceipt {
            id: id.into(),
            source: plan.source,
            fingerprint: plan.fingerprint,
            imported_files: plan
                .files
                .len()
                .try_into()
                .map_err(|_| AppError::new("migration-size", "Too many files"))?,
            imported_bytes: total.to_string(),
            pending_roots,
            phase: "imported-requires-adapter-validation".into(),
            configuration,
        };
        tx.execute(
            "UPDATE operations SET result=?2 WHERE id=?1",
            params![
                id,
                serde_json::to_string(&OperationResult::Migration(receipt.clone()))?
            ],
        )?;
        tx.commit()?;
        Ok(receipt)
    }
    pub fn legacy_artifact(&self, id: &str, path: &str) -> Result<Vec<u8>> {
        // Rust adapters only; never expose arbitrary cached/provider bytes through a generic IPC command.
        let (expected, bytes): (String, Vec<u8>) = self.db()?.query_row(
            "SELECT sha256,body FROM legacy_artifacts WHERE import_id=?1 AND path=?2",
            params![id, path],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if hash(&bytes) != expected {
            return Err(AppError::new(
                "migration-archive-corrupt",
                "Imported data failed its original hash check",
            ));
        }
        Ok(bytes)
    }
    pub fn preferences(&self, key: &str) -> Result<serde_json::Value> {
        let body: Option<String> = self
            .db()?
            .query_row("SELECT body FROM preferences WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?;
        body.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .unwrap_or_else(|| Ok(serde_json::json!({})))
    }
    pub fn save_preference(&self, key: &str, value: &serde_json::Value) -> Result<()> {
        let db = self.db()?;
        self.writable()?;
        db.execute("INSERT INTO preferences(key,body) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET body=excluded.body",params![key,serde_json::to_string(value)?])?;
        Ok(())
    }
    pub fn update_progress(&self, progress: &crate::update::UpdateProgress) -> Result<()> {
        valid_id(&progress.operation_id)?;
        let mut db = self.db()?;
        let tx = db.transaction()?;
        let fingerprint = hash(b"application-update-v1");
        let old: Option<String> = tx
            .query_row(
                "SELECT fingerprint FROM operations WHERE id=?1",
                [&progress.operation_id],
                |row| row.get(0),
            )
            .optional()?;
        if old.as_ref().is_some_and(|old| old != &fingerprint)
            || tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM tasks WHERE id=?1)",
                [&progress.operation_id],
                |row| row.get::<_, bool>(0),
            )?
        {
            return Err(AppError::new(
                "operation-conflict",
                "Update ID belongs to another operation",
            ));
        }
        tx.execute("INSERT INTO operations(id,fingerprint,result) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET result=excluded.result",params![progress.operation_id,fingerprint,serde_json::to_string(&OperationResult::Update(progress.clone()))?])?;
        tx.execute("INSERT INTO preferences(key,body) VALUES('update-state',?1) ON CONFLICT(key) DO UPDATE SET body=excluded.body",[serde_json::to_string(progress)?])?;
        tx.commit()?;
        Ok(())
    }
    pub fn snapshot_database(&self, path: &Path) -> Result<()> {
        if path.exists() {
            return Err(AppError::new(
                "update-backup-exists",
                "Database backup must be immutable",
            ));
        }
        let path = path
            .to_str()
            .ok_or_else(|| AppError::new("update-backup-path", "Backup path must be Unicode"))?;
        self.db()?.execute("VACUUM main INTO ?1", [path])?;
        Ok(())
    }
    pub fn freeze_for_update(&self) -> Result<()> {
        let db = self.db()?;
        let mut statement = db.prepare("SELECT body FROM tasks")?;
        for body in statement.query_map([], |row| row.get::<_, String>(0))? {
            let task: Task = serde_json::from_str(&body?)?;
            if !task.state.terminal() {
                return Err(AppError::new(
                    "active-task",
                    "Finish or cancel tasks before installing the update",
                ));
            }
        }
        self.maintenance
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
    pub fn unfreeze_after_update(&self) {
        self.maintenance
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }
    fn writable(&self) -> Result<()> {
        if self.maintenance.load(std::sync::atomic::Ordering::SeqCst) {
            Err(AppError::new(
                "update-in-progress",
                "Application state is frozen for installation",
            ))
        } else {
            Ok(())
        }
    }
    pub fn configuration(&self) -> Result<Configuration> {
        let body: Option<String> = self
            .db()?
            .query_row("SELECT body FROM configuration WHERE id=1", [], |r| {
                r.get(0)
            })
            .optional()?;
        body.map(|b| serde_json::from_str(&b).map_err(Into::into))
            .unwrap_or_else(|| Ok(Configuration::default()))
    }
    pub fn operation_result(&self, id: &str) -> Result<OperationResult> {
        let body: Option<String> = self
            .db()?
            .query_row("SELECT result FROM operations WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        if let Some(body) = body {
            if let Ok(value) = serde_json::from_str::<OperationResult>(&body) {
                return Ok(value);
            }
            if let Ok(value) = serde_json::from_str::<Configuration>(&body) {
                return Ok(OperationResult::Configuration(value));
            }
            return Ok(OperationResult::Task(serde_json::from_str(&body)?));
        }
        let body: Option<String> = self
            .db()?
            .query_row("SELECT body FROM tasks WHERE id=?1", [id], |r| r.get(0))
            .optional()?;
        match body {
            Some(body) => Ok(OperationResult::Task(serde_json::from_str(&body)?)),
            None => Err(AppError::new(
                "operation-not-found",
                "No persisted result for this operation ID",
            )),
        }
    }
    pub fn configure(&self, operation_id: &str, mut value: Configuration) -> Result<Configuration> {
        valid_id(operation_id)?;
        let fingerprint = hash(serde_json::to_string(&value)?.as_bytes());
        // Recognize a retry before validating paths that may have gone offline since success.
        if let Some(result) = self.operation(operation_id, &fingerprint)? {
            return Ok(serde_json::from_str(&result)?);
        }
        let mut roots = vec![];
        for root in &mut value.roots {
            valid_id(&root.id)?;
            let real = paths::checked(Path::new(&root.path))?;
            if !real.is_dir() {
                return Err(
                    AppError::new("invalid-root", "Library root must be a directory")
                        .at(real.display()),
                );
            }
            if roots
                .iter()
                .any(|(id, p, space): &(String, std::path::PathBuf, Space)| {
                    id == &root.id
                        || (real.starts_with(p) || p.starts_with(&real))
                            && !(real == *p && root.space != *space)
                })
            {
                return Err(AppError::new(
                    "overlapping-roots",
                    "Root IDs and physical roots must not overlap",
                ));
            }
            root.path = real
                .to_str()
                .ok_or_else(|| AppError::new("invalid-encoding", "Root path is not valid Unicode"))?
                .into();
            roots.push((root.id.clone(), real, root.space.clone()));
        }
        let mut db = self.db()?;
        self.writable()?;
        let tx = db.transaction()?;
        if let Some((old, result)) = tx
            .query_row(
                "SELECT fingerprint,result FROM operations WHERE id=?1",
                [operation_id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
        {
            if old != fingerprint {
                return Err(AppError::new(
                    "operation-conflict",
                    "Operation ID was used for different input",
                ));
            }
            return Ok(serde_json::from_str(&result)?);
        }
        if tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id=?1)",
            [operation_id],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(AppError::new(
                "operation-conflict",
                "Operation ID already belongs to a task",
            ));
        }
        let existing: Option<String> = tx
            .query_row("SELECT body FROM configuration WHERE id=1", [], |r| {
                r.get(0)
            })
            .optional()?;
        let current: Configuration = existing
            .map(|s| serde_json::from_str(&s))
            .transpose()?
            .unwrap_or_default();
        if current.revision != value.revision {
            return Err(AppError::new(
                "configuration-conflict",
                "Reload settings before saving",
            ));
        }
        // Configuration must not invalidate an in-flight root snapshot.
        let mut stmt = tx.prepare("SELECT body FROM tasks")?;
        for body in stmt.query_map([], |r| r.get::<_, String>(0))? {
            let task: Task = serde_json::from_str(&body?)?;
            if !task.state.terminal() && current.roots != value.roots {
                return Err(AppError::new(
                    "active-task",
                    "Cancel or finish library tasks before changing roots",
                ));
            }
        }
        drop(stmt);
        value.revision = value.revision.checked_add(1).ok_or_else(|| {
            AppError::new("revision-overflow", "Configuration revision exhausted")
        })?;
        let serialized = serde_json::to_string(&value)?;
        tx.execute("INSERT INTO configuration VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET body=excluded.body",[&serialized])?;
        tx.execute(
            "INSERT INTO operations VALUES(?1,?2,?3)",
            params![operation_id, fingerprint, serialized],
        )?;
        let keep: Vec<String> = value.roots.iter().map(|r| r.id.clone()).collect();
        let mut stmt = tx.prepare("SELECT DISTINCT root_id FROM items")?;
        let old = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);
        for root in old {
            if !keep.contains(&root)
                || current.roots.iter().find(|r| r.id == root)
                    != value.roots.iter().find(|r| r.id == root)
            {
                tx.execute("DELETE FROM items WHERE root_id=?1", [root])?;
            }
        }
        tx.commit()?;
        Ok(value)
    }
    fn operation(&self, id: &str, fingerprint: &str) -> Result<Option<String>> {
        let value = self
            .db()?
            .query_row(
                "SELECT fingerprint,result FROM operations WHERE id=?1",
                [id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?;
        match value {
            Some((old, result)) if old == fingerprint => Ok(Some(result)),
            Some(_) => Err(AppError::new(
                "operation-conflict",
                "Operation ID was used for different input",
            )),
            None => Ok(None),
        }
    }
    pub fn submit(&self, request: ScanRequest) -> Result<Task> {
        valid_id(&request.operation_id)?;
        if request.root_ids.is_empty() {
            return Err(AppError::new(
                "empty-scope",
                "Explicitly select library roots",
            ));
        }
        let fingerprint = hash(serde_json::to_string(&request)?.as_bytes());
        if let Some((old, body)) = self
            .db()?
            .query_row(
                "SELECT fingerprint,body FROM tasks WHERE id=?1",
                [&request.operation_id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
        {
            if old != fingerprint {
                return Err(AppError::new(
                    "operation-conflict",
                    "Task ID was used for different input",
                ));
            }
            return Ok(serde_json::from_str(&body)?);
        }
        let config = self.configuration()?;
        let mut roots = vec![];
        for id in &request.root_ids {
            let root = config
                .roots
                .iter()
                .find(|r| &r.id == id && r.space == request.space)
                .ok_or_else(|| {
                    AppError::new(
                        "invalid-scope",
                        "Root does not belong to selected media space",
                    )
                })?;
            if roots.contains(root) {
                return Err(AppError::new("invalid-scope", "Duplicate root"));
            }
            roots.push(root.clone());
        }
        let task = Task {
            id: request.operation_id.clone(),
            state: TaskState::Requested,
            locale: config.locale,
            space: request.space,
            roots,
            attempt: 0,
            processed: 0,
            errors: 0,
            current_path: None,
            failure: None,
        };
        let db = self.db()?;
        self.writable()?;
        let latest: String =
            db.query_row("SELECT body FROM configuration WHERE id=1", [], |r| {
                r.get(0)
            })?;
        if serde_json::from_str::<Configuration>(&latest)?.revision != config.revision {
            return Err(AppError::new(
                "configuration-conflict",
                "Settings changed while preparing task",
            ));
        }
        if let Some((old, body)) = db
            .query_row(
                "SELECT fingerprint,body FROM tasks WHERE id=?1",
                [&task.id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
        {
            if old != fingerprint {
                return Err(AppError::new(
                    "operation-conflict",
                    "Task ID was used for different input",
                ));
            }
            return Ok(serde_json::from_str(&body)?);
        }
        if db.query_row(
            "SELECT EXISTS(SELECT 1 FROM operations WHERE id=?1)",
            [&task.id],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(AppError::new(
                "operation-conflict",
                "Task ID already belongs to another operation",
            ));
        }
        db.execute(
            "INSERT INTO tasks VALUES(?1,?2,?3)",
            params![task.id, fingerprint, serde_json::to_string(&task)?],
        )?;
        Ok(task)
    }
    fn save_task(&self, task: &Task) -> Result<()> {
        self.db()?.execute(
            "UPDATE tasks SET body=?2 WHERE id=?1",
            params![task.id, serde_json::to_string(task)?],
        )?;
        Ok(())
    }
    pub fn tasks(&self) -> Result<Vec<Task>> {
        let db = self.db()?;
        let mut stmt = db.prepare("SELECT body FROM tasks ORDER BY rowid DESC")?;
        let result = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .map(|s| Ok(serde_json::from_str(&s?)?))
            .collect();
        result
    }
    pub fn task(&self, id: &str) -> Result<Task> {
        let body: String =
            self.db()?
                .query_row("SELECT body FROM tasks WHERE id=?1", [id], |r| r.get(0))?;
        Ok(serde_json::from_str(&body)?)
    }
    pub fn control(&self, request: TaskControl) -> Result<Task> {
        valid_id(&request.operation_id)?;
        let fingerprint = hash(serde_json::to_string(&request)?.as_bytes());
        let id = &request.task_id;
        let state = request.state;
        let mut db = self.db()?;
        self.writable()?;
        let tx = db.transaction()?;
        if let Some((old, result)) = tx
            .query_row(
                "SELECT fingerprint,result FROM operations WHERE id=?1",
                [&request.operation_id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
        {
            if old != fingerprint {
                return Err(AppError::new(
                    "operation-conflict",
                    "Control ID already used",
                ));
            }
            return Ok(serde_json::from_str(&result)?);
        }
        if tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id=?1)",
            [&request.operation_id],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(AppError::new(
                "operation-conflict",
                "Control ID already belongs to a task",
            ));
        }
        let body: String =
            tx.query_row("SELECT body FROM tasks WHERE id=?1", [id], |r| r.get(0))?;
        let mut task: Task = serde_json::from_str(&body)?;
        let valid = match state {
            TaskState::Paused => matches!(task.state, TaskState::Requested | TaskState::Running),
            TaskState::Cancelled => !task.state.terminal(),
            TaskState::Requested => {
                matches!(task.state, TaskState::Paused | TaskState::Interrupted)
            }
            _ => false,
        };
        if !valid && task.state != state {
            return Err(AppError::new(
                "invalid-transition",
                "Task cannot enter the requested state",
            ));
        }
        task.state = state;
        let body = serde_json::to_string(&task)?;
        tx.execute("UPDATE tasks SET body=?2 WHERE id=?1", params![id, body])?;
        tx.execute(
            "INSERT INTO operations VALUES(?1,?2,?3)",
            params![request.operation_id, fingerprint, body],
        )?;
        tx.commit()?;
        Ok(task)
    }
    /// A stable, completed catalog snapshot. No item deserialization on unchanged
    /// revisions; active/paused/interrupted scans never publish a partial library.
    pub fn publication_snapshot(
        &self,
        after: Option<i64>,
    ) -> Result<Option<(i64, Vec<MediaItem>)>> {
        let db = self.db()?;
        let revision: i64 =
            db.query_row("SELECT value FROM catalog_revision WHERE id=1", [], |r| {
                r.get(0)
            })?;
        if after == Some(revision) {
            return Ok(None);
        }
        let active: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE json_extract(body,'$.state') IN ('requested','running','paused','interrupted'))", [], |r| r.get(0))?;
        if active {
            return Ok(None);
        }
        let mut statement = db.prepare("SELECT body FROM items ORDER BY id")?;
        let mut items = Vec::new();
        for body in statement.query_map([], |row| row.get::<_, String>(0))? {
            items.push(serde_json::from_str(&body?)?);
        }
        Ok(Some((revision, items)))
    }
    pub fn all_items(&self) -> Result<Vec<MediaItem>> {
        let db = self.db()?;
        let mut statement = db.prepare("SELECT body FROM items ORDER BY id")?;
        let mut items = Vec::new();
        for body in statement.query_map([], |row| row.get::<_, String>(0))? {
            items.push(serde_json::from_str(&body?)?);
        }
        Ok(items)
    }
    pub fn path_mappings(&self) -> Result<crate::emby_libraries::MappingSettings> {
        let value = self.preferences("emby-path-mappings")?;
        if value.get("revision").is_none() {
            return Ok(Default::default());
        }
        serde_json::from_value(value).map_err(Into::into)
    }
    pub fn set_path_mappings(
        &self,
        id: &str,
        mut value: crate::emby_libraries::MappingSettings,
    ) -> Result<crate::emby_libraries::MappingSettings> {
        valid_id(id)?;
        let fingerprint = hash(&serde_json::to_vec(&value)?);
        if let Some(result) = self.operation(id, &fingerprint)? {
            if let OperationResult::PathMappings(value) = serde_json::from_str(&result)? {
                return Ok(value);
            }
            return Err(AppError::new(
                "operation-conflict",
                "ID belongs to another operation",
            ));
        }
        if value.mappings.len() > 100 {
            return Err(AppError::new("path-mapping", "Too many mappings"));
        }
        for mapping in &value.mappings {
            crate::emby_libraries::mapped_path(&mapping.server_prefix, &value.mappings)?;
        }
        let mut db = self.db()?;
        self.writable()?;
        let tx = db.transaction()?;
        if tx.query_row("SELECT EXISTS(SELECT 1 FROM operations WHERE id=?1 UNION ALL SELECT 1 FROM tasks WHERE id=?1)",[id],|r|r.get::<_,bool>(0))?{return Err(AppError::new("operation-conflict","ID is already in use"));}
        let previous: Option<String> = tx
            .query_row(
                "SELECT body FROM preferences WHERE key='emby-path-mappings'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let previous: crate::emby_libraries::MappingSettings = previous
            .map(|s| serde_json::from_str(&s))
            .transpose()?
            .unwrap_or_default();
        if previous.revision != value.revision {
            return Err(AppError::new(
                "path-mapping-conflict",
                "Mappings changed; reload before saving",
            ));
        }
        value.revision = value
            .revision
            .checked_add(1)
            .ok_or_else(|| AppError::new("revision-overflow", "Mapping revision exhausted"))?;
        tx.execute("INSERT INTO preferences(key,body) VALUES('emby-path-mappings',?1) ON CONFLICT(key) DO UPDATE SET body=excluded.body",[serde_json::to_string(&value)?])?;
        tx.execute(
            "INSERT INTO operations(id,fingerprint,result) VALUES(?1,?2,?3)",
            params![
                id,
                fingerprint,
                serde_json::to_string(&OperationResult::PathMappings(value.clone()))?
            ],
        )?;
        tx.commit()?;
        Ok(value)
    }

    pub fn lifecycle_settings(&self) -> Result<crate::lifecycle::Settings> {
        let value = self.preferences("lifecycle-settings")?;
        if value.get("revision").is_none() {
            return Ok(crate::lifecycle::Settings::default());
        }
        serde_json::from_value(value).map_err(Into::into)
    }
    pub fn prepare_lifecycle(
        &self,
        id: &str,
        desired: crate::lifecycle::Settings,
        native_before: bool,
    ) -> Result<crate::lifecycle::SettingsOperation> {
        valid_id(id)?;
        let fingerprint = hash(&serde_json::to_vec(&desired)?);
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
                    "Settings ID belongs to different input",
                ));
            }
            if let OperationResult::Lifecycle(value) = serde_json::from_str(&body)? {
                return Ok(value);
            }
            return Err(AppError::new(
                "operation-conflict",
                "Settings ID belongs to another action",
            ));
        }
        if tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id=?1)",
            [id],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(AppError::new(
                "operation-conflict",
                "Settings ID belongs to a task",
            ));
        }
        let pending: Option<String> = tx
            .query_row(
                "SELECT body FROM preferences WHERE key='lifecycle-pending'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if pending.is_some_and(|s| s != "null") {
            return Err(AppError::new(
                "settings-recovery-required",
                "Reconcile the previous settings operation first",
            ));
        }
        let old: Option<String> = tx
            .query_row(
                "SELECT body FROM preferences WHERE key='lifecycle-settings'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let previous: crate::lifecycle::Settings = old
            .map(|s| serde_json::from_str(&s))
            .transpose()?
            .unwrap_or_default();
        if desired.revision != previous.revision {
            return Err(AppError::new(
                "settings-conflict",
                "Settings changed; refresh before applying",
            ));
        }
        let operation = crate::lifecycle::SettingsOperation {
            id: id.into(),
            phase: "planned".into(),
            desired,
            previous,
            native_before,
            error: None,
        };
        tx.execute(
            "INSERT INTO operations(id,fingerprint,result) VALUES(?1,?2,?3)",
            params![
                id,
                fingerprint,
                serde_json::to_string(&OperationResult::Lifecycle(operation.clone()))?
            ],
        )?;
        tx.execute("INSERT INTO preferences(key,body) VALUES('lifecycle-pending',?1) ON CONFLICT(key) DO UPDATE SET body=excluded.body",[serde_json::to_string(id)?])?;
        tx.commit()?;
        Ok(operation)
    }
    pub fn finish_lifecycle(
        &self,
        id: &str,
        phase: &str,
        error: Option<AppError>,
    ) -> Result<crate::lifecycle::SettingsOperation> {
        if !matches!(phase, "committed" | "failed" | "interrupted") {
            return Err(AppError::new(
                "settings-phase",
                "Invalid settings completion phase",
            ));
        }
        let mut db = self.db()?;
        self.writable()?;
        let tx = db.transaction()?;
        let body: String =
            tx.query_row("SELECT result FROM operations WHERE id=?1", [id], |r| {
                r.get(0)
            })?;
        let OperationResult::Lifecycle(mut operation) = serde_json::from_str(&body)? else {
            return Err(AppError::new(
                "operation-conflict",
                "Not a settings operation",
            ));
        };
        if operation.phase != "planned" {
            return Ok(operation);
        }
        if phase == "committed" {
            operation.desired.revision =
                operation.previous.revision.checked_add(1).ok_or_else(|| {
                    AppError::new("settings-revision", "Settings revision exhausted")
                })?;
            tx.execute("INSERT INTO preferences(key,body) VALUES('lifecycle-settings',?1) ON CONFLICT(key) DO UPDATE SET body=excluded.body",[serde_json::to_string(&operation.desired)?])?;
        }
        operation.phase = phase.into();
        operation.error = error;
        tx.execute(
            "UPDATE operations SET result=?2 WHERE id=?1",
            params![
                id,
                serde_json::to_string(&OperationResult::Lifecycle(operation.clone()))?
            ],
        )?;
        tx.execute(
            "UPDATE preferences SET body='null' WHERE key='lifecycle-pending'",
            [],
        )?;
        tx.commit()?;
        Ok(operation)
    }
    pub fn ui_state(&self) -> Result<crate::ui::UiState> {
        let value = self.preferences("library-view")?;
        if value.get("revision").is_none() {
            return Ok(crate::ui::UiState::default());
        }
        serde_json::from_value(value).map_err(Into::into)
    }
    pub fn save_ui_state(
        &self,
        id: &str,
        mut state: crate::ui::UiState,
    ) -> Result<crate::ui::UiReceipt> {
        valid_id(id)?;
        state.validate()?;
        let fingerprint = hash(&serde_json::to_vec(&state)?);
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
                    "View operation ID belongs to another change",
                ));
            }
            if let OperationResult::Ui(value) = serde_json::from_str(&body)? {
                return Ok(value);
            }
            return Err(AppError::new(
                "operation-conflict",
                "View operation ID belongs to another action",
            ));
        }
        if tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id=?1)",
            [id],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(AppError::new(
                "operation-conflict",
                "View operation ID belongs to a task",
            ));
        }
        let old: Option<String> = tx
            .query_row(
                "SELECT body FROM preferences WHERE key='library-view'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let previous: crate::ui::UiState = old
            .map(|s| serde_json::from_str(&s))
            .transpose()?
            .unwrap_or_default();
        if state.revision != previous.revision {
            return Err(AppError::new(
                "view-state-conflict",
                "View state changed in another request",
            ));
        }
        state.revision = state
            .revision
            .checked_add(1)
            .ok_or_else(|| AppError::new("view-state-revision", "View revision exhausted"))?;
        tx.execute("INSERT INTO preferences(key,body) VALUES('library-view',?1) ON CONFLICT(key) DO UPDATE SET body=excluded.body",[serde_json::to_string(&state)?])?;
        tx.execute(
            "INSERT INTO operations(id,fingerprint,result) VALUES(?1,?2,?3)",
            params![
                id,
                fingerprint,
                serde_json::to_string(&OperationResult::Ui(crate::ui::UiReceipt {
                    revision: state.revision
                }))?
            ],
        )?;
        tx.commit()?;
        Ok(crate::ui::UiReceipt {
            revision: state.revision,
        })
    }
    pub fn browse(&self, space: Space, view: crate::ui::LibraryView) -> Result<CatalogPage> {
        let mut items: Vec<_> = self
            .all_items()?
            .into_iter()
            .filter(|i| crate::ui::matches(i, &space, &view))
            .collect();
        crate::ui::sort(&mut items, &view);
        Ok(CatalogPage {
            total: items.len().try_into().unwrap_or(u32::MAX),
            items: items
                .into_iter()
                .skip(view.offset as usize)
                .take(100)
                .collect(),
        })
    }
    pub fn query(&self, query: CatalogQuery) -> Result<CatalogPage> {
        let db = self.db()?;
        let mut stmt = db.prepare("SELECT body FROM items ORDER BY id")?;
        let search = query.search.to_lowercase();
        let mut items = vec![];
        for body in stmt.query_map([], |r| r.get::<_, String>(0))? {
            let item: MediaItem = serde_json::from_str(&body?)?;
            if item.space != query.space || query.only_errors && item.error.is_none() {
                continue;
            }
            if !search.is_empty()
                && !format!("{} {} {} {}", item.title, item.year, item.imdb, item.path)
                    .to_lowercase()
                    .contains(&search)
            {
                continue;
            }
            items.push(item);
        }
        items.sort_by(|a, b| (&a.title, &a.path).cmp(&(&b.title, &b.path)));
        Ok(CatalogPage {
            total: items.len().try_into().unwrap_or(u32::MAX),
            items: items
                .into_iter()
                .skip(query.offset as usize)
                .take(query.limit.clamp(1, 500) as usize)
                .collect(),
        })
    }
    pub fn item(&self, id: &str) -> Result<MediaItem> {
        let body: String =
            self.db()?
                .query_row("SELECT body FROM items WHERE id=?1", [id], |r| r.get(0))?;
        Ok(serde_json::from_str(&body)?)
    }
    fn checkpoint(&self, id: &str, stop: &impl Fn() -> bool) -> Result<bool> {
        if stop() {
            let mut task = self.task(id)?;
            if task.state == TaskState::Running {
                task.state = TaskState::Interrupted;
                self.save_task(&task)?;
            }
            return Ok(false);
        }
        Ok(self.task(id)?.state == TaskState::Running)
    }
    // Called by exactly one owned worker. Pause/cancel apply between bounded file reads.
    pub fn run_next(
        &self,
        stop: impl Fn() -> bool,
        progress: impl Fn(&Task),
    ) -> Result<Option<Task>> {
        let _worker = self.worker.try_lock().map_err(|_| {
            AppError::new("worker-busy", "Only one index worker may own this store")
        })?;
        let task = {
            let mut db = self.db()?;
            let tx = db.transaction()?;
            let mut stmt = tx.prepare("SELECT body FROM tasks ORDER BY rowid")?;
            let mut selected = None;
            for row in stmt.query_map([], |r| r.get::<_, String>(0))? {
                let t: Task = serde_json::from_str(&row?)?;
                if t.state == TaskState::Requested {
                    selected = Some(t);
                    break;
                }
            }
            drop(stmt);
            let Some(mut task) = selected else {
                return Ok(None);
            };
            task.state = TaskState::Running;
            task.processed = 0;
            task.errors = 0;
            task.failure = None;
            task.attempt = task.attempt.checked_add(1).ok_or_else(|| {
                AppError::new("attempt-overflow", "Task attempt counter exhausted")
            })?;
            tx.execute(
                "UPDATE tasks SET body=?2 WHERE id=?1",
                params![task.id, serde_json::to_string(&task)?],
            )?;
            tx.commit()?;
            task
        };
        progress(&task);
        let scan_id = format!("{}:{}", task.id, task.attempt);
        for root in &task.roots {
            let mut stack = vec![std::path::PathBuf::from(&root.path)];
            let mut root_failed = false;
            while let Some(path) = stack.pop() {
                if !self.checkpoint(&task.id, &stop)? {
                    return Ok(Some(self.task(&task.id)?));
                }
                let result = paths::within(Path::new(&root.path), &path).and_then(|real| {
                    if real.is_dir() {
                        for entry in std::fs::read_dir(&real)
                            .map_err(|e| AppError::new("directory-read", e).at(real.display()))?
                        {
                            stack.push(
                                entry
                                    .map_err(|e| {
                                        AppError::new("directory-read", e).at(real.display())
                                    })?
                                    .path(),
                            );
                        }
                        return Ok(None);
                    }
                    if !real
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("nfo"))
                    {
                        return Ok(None);
                    }
                    let (real, raw) = library::read_bytes(root, &real)?;
                    let previous = self.item(&hash(real.to_string_lossy().as_bytes()));
                    let item = match previous {
                        Ok(item)
                            if item.parser_revision == library::PARSER_REVISION
                                && item.source_hash == hash(&raw)
                                && item.error.is_none()
                                && item.root_id == root.id
                                && item.space == root.space =>
                        {
                            item
                        }
                        _ => library::parse(root, &real, &raw)?,
                    };
                    let relevant = match item.kind.as_str() {
                        "Movie" => root.space == Space::Movie,
                        "Series" | "Season" | "Episode" => root.space == Space::Tv,
                        _ => true,
                    };
                    Ok(relevant.then_some(item))
                });
                let item = match result {
                    Ok(Some(item)) => Some(item),
                    Ok(None) => None,
                    Err(mut error) => {
                        error.operation_id = Some(task.id.clone());
                        root_failed = true;
                        let mut item = library::empty(root, &path);
                        item.id = hash(format!("error:{}:{}", root.id, item.path).as_bytes());
                        item.error = Some(error);
                        Some(item)
                    }
                };
                if let Some(item) = item {
                    // Read fresh control state under the same lock as the progress update.
                    let mut db = self.db()?;
                    let tx = db.transaction()?;
                    let body: String =
                        tx.query_row("SELECT body FROM tasks WHERE id=?1", [&task.id], |r| {
                            r.get(0)
                        })?;
                    let mut current: Task = serde_json::from_str(&body)?;
                    if current.state != TaskState::Running {
                        return Ok(Some(current));
                    }
                    current.processed += 1;
                    current.errors += u32::from(item.error.is_some());
                    current.current_path = Some(item.path.clone());
                    tx.execute("INSERT INTO items VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET root_id=excluded.root_id,seen_task=excluded.seen_task,body=excluded.body",params![item.id,root.id,scan_id,serde_json::to_string(&item)?])?;
                    tx.execute(
                        "UPDATE tasks SET body=?2 WHERE id=?1",
                        params![task.id, serde_json::to_string(&current)?],
                    )?;
                    tx.commit()?;
                    drop(db);
                    progress(&current);
                }
            }
            if !self.checkpoint(&task.id, &stop)? {
                return Ok(Some(self.task(&task.id)?));
            }
            if !root_failed {
                self.db()?.execute(
                    "DELETE FROM items WHERE root_id=?1 AND seen_task<>?2",
                    params![root.id, scan_id],
                )?;
            }
        }
        // A cancel arriving after the last file must not become a completed task.
        let mut db = self.db()?;
        let tx = db.transaction()?;
        let body: String = tx.query_row("SELECT body FROM tasks WHERE id=?1", [&task.id], |r| {
            r.get(0)
        })?;
        let mut result: Task = serde_json::from_str(&body)?;
        if result.state == TaskState::Running {
            result.state = if result.errors == 0 {
                TaskState::Completed
            } else {
                TaskState::Failed
            };
            result.current_path = None;
            tx.execute(
                "UPDATE tasks SET body=?2 WHERE id=?1",
                params![result.id, serde_json::to_string(&result)?],
            )?;
        }
        tx.commit()?;
        drop(db);
        progress(&result);
        Ok(Some(result))
    }
}
