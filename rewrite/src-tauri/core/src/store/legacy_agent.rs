use super::*;
use crate::legacy_agent::Snapshot;

impl Store {
    pub fn backup_legacy_agent(&self, value: &Snapshot) -> Result<String> {
        let fingerprint = value.fingerprint()?;
        let key = format!("legacy-agent-backup:{fingerprint}");
        let body = serde_json::to_string(value)?;
        let mut db = self.db()?;
        self.writable()?;
        let tx = db.transaction()?;
        tx.execute(
            "INSERT INTO preferences(key,body) VALUES(?1,?2) ON CONFLICT(key) DO NOTHING",
            params![key, body],
        )?;
        let saved: String =
            tx.query_row("SELECT body FROM preferences WHERE key=?1", [&key], |r| {
                r.get(0)
            })?;
        if saved != body {
            return Err(AppError::new(
                "legacy-agent-backup-conflict",
                "旧状态文件备份冲突，未覆盖",
            ));
        }
        tx.commit()?;
        drop(db);
        self.legacy_agent_backup(&fingerprint)?;
        Ok(fingerprint)
    }
    pub fn legacy_agent_backup(&self, fingerprint: &str) -> Result<Snapshot> {
        if fingerprint.len() != 64 || !fingerprint.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(AppError::new(
                "legacy-agent-backup-id",
                "旧状态文件备份编号无效",
            ));
        }
        let body: Option<String> = self
            .db()?
            .query_row(
                "SELECT body FROM preferences WHERE key=?1",
                [format!("legacy-agent-backup:{fingerprint}")],
                |r| r.get(0),
            )
            .optional()?;
        let body = body
            .ok_or_else(|| AppError::new("legacy-agent-backup-missing", "旧状态文件备份不存在"))?;
        if hash(body.as_bytes()) != fingerprint {
            return Err(AppError::new(
                "legacy-agent-backup-corrupt",
                "旧状态文件备份内容已改变",
            ));
        }
        let value: Snapshot = serde_json::from_str(&body)?;
        value.validate()?;
        Ok(value)
    }
}
