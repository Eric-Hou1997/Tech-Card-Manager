use super::*;
use crate::legacy_startup::{RunBackup, RunPhase, RunReceipt};

impl Store {
    /// Claim once before touching HKCU. An interrupted Removing receipt is
    /// deliberately not replayable: absence cannot prove who deleted a value.
    pub fn begin_legacy_run_removal(&self, id: &str, backup: &str) -> Result<RunReceipt> {
        valid_id(id)?;
        self.legacy_run_backup(backup)?;
        let receipt = RunReceipt {
            id: id.into(),
            backup: backup.into(),
            phase: RunPhase::Removing,
        };
        let mut db = self.db()?;
        self.writable()?;
        let tx = db.transaction()?;
        let inserted = tx.execute(
            "INSERT INTO preferences(key,body) VALUES(?1,?2) ON CONFLICT(key) DO NOTHING",
            params![
                format!("legacy-run-operation:{id}"),
                serde_json::to_string(&receipt)?
            ],
        )?;
        if inserted != 1 {
            return Err(AppError::new(
                "legacy-startup-operation-exists",
                "旧登录项操作已存在，请先检查原回执",
            ));
        }
        tx.commit()?;
        Ok(receipt)
    }
    pub fn legacy_run_receipt(&self, id: &str) -> Result<RunReceipt> {
        valid_id(id)?;
        let body: Option<String> = self
            .db()?
            .query_row(
                "SELECT body FROM preferences WHERE key=?1",
                [format!("legacy-run-operation:{id}")],
                |row| row.get(0),
            )
            .optional()?;
        let receipt: RunReceipt = serde_json::from_str(&body.ok_or_else(|| {
            AppError::new("legacy-startup-operation-missing", "旧登录项操作回执不存在")
        })?)?;
        if receipt.id != id {
            return Err(AppError::new(
                "legacy-startup-operation-conflict",
                "旧登录项操作回执不匹配",
            ));
        }
        self.legacy_run_backup(&receipt.backup)?;
        Ok(receipt)
    }
    /// Compare the entire saved receipt; stale callers cannot regress a newer
    /// result or claim an absent/uncertain deletion as their own removal.
    pub fn advance_legacy_run(&self, expected: &RunReceipt, phase: RunPhase) -> Result<RunReceipt> {
        valid_id(&expected.id)?;
        self.legacy_run_backup(&expected.backup)?;
        if !matches!(
            (expected.phase, phase),
            (RunPhase::Removing, RunPhase::Removed | RunPhase::Absent)
                | (RunPhase::Removed, RunPhase::Restoring)
                | (RunPhase::Restoring, RunPhase::Restored)
        ) {
            return Err(AppError::new(
                "legacy-startup-operation-phase",
                "旧登录项操作阶段不允许此操作",
            ));
        }
        let next = RunReceipt {
            phase,
            ..expected.clone()
        };
        let mut db = self.db()?;
        self.writable()?;
        let tx = db.transaction()?;
        let changed = tx.execute(
            "UPDATE preferences SET body=?1 WHERE key=?2 AND body=?3",
            params![
                serde_json::to_string(&next)?,
                format!("legacy-run-operation:{}", expected.id),
                serde_json::to_string(expected)?
            ],
        )?;
        if changed != 1 {
            return Err(AppError::new(
                "legacy-startup-operation-conflict",
                "旧登录项操作回执已改变，请重新检查",
            ));
        }
        tx.commit()?;
        Ok(next)
    }
    /// Immutable content-addressed backup, committed and read back before any
    /// registry deletion. These records are not executed or auto-restored.
    pub fn backup_legacy_run(&self, value: &RunBackup) -> Result<String> {
        let fingerprint = value.fingerprint()?;
        let key = format!("legacy-run-backup:{fingerprint}");
        let body = serde_json::to_string(value)?;
        let mut db = self.db()?;
        self.writable()?;
        let tx = db.transaction()?;
        tx.execute(
            "INSERT INTO preferences(key,body) VALUES(?1,?2) ON CONFLICT(key) DO NOTHING",
            params![key, body],
        )?;
        let saved: String =
            tx.query_row("SELECT body FROM preferences WHERE key=?1", [&key], |row| {
                row.get(0)
            })?;
        if saved != body {
            return Err(AppError::new(
                "legacy-startup-backup-conflict",
                "旧登录项备份校验失败，未覆盖已有备份",
            ));
        }
        tx.commit()?;
        drop(db);
        if self.legacy_run_backup(&fingerprint)?.fingerprint()? != fingerprint {
            return Err(AppError::new(
                "legacy-startup-backup-conflict",
                "旧登录项备份校验失败",
            ));
        }
        Ok(fingerprint)
    }
    pub fn legacy_run_backup(&self, fingerprint: &str) -> Result<RunBackup> {
        if fingerprint.len() != 64 || !fingerprint.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(AppError::new(
                "legacy-startup-backup-id",
                "旧登录项备份编号无效",
            ));
        }
        let body: Option<String> = self
            .db()?
            .query_row(
                "SELECT body FROM preferences WHERE key=?1",
                [format!("legacy-run-backup:{fingerprint}")],
                |row| row.get(0),
            )
            .optional()?;
        let body = body
            .ok_or_else(|| AppError::new("legacy-startup-backup-missing", "旧登录项备份不存在"))?;
        if hash(body.as_bytes()) != fingerprint {
            return Err(AppError::new(
                "legacy-startup-backup-corrupt",
                "旧登录项备份内容已改变",
            ));
        }
        let value: RunBackup = serde_json::from_str(&body)?;
        value.path()?;
        Ok(value)
    }
}
