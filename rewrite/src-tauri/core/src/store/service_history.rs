use super::*;
use crate::card_service::ServiceStatus;
impl Store {
    /// Renewals and status-query times never become new start timestamps.
    pub fn service_status_with_history(&self, mut status: ServiceStatus) -> Result<ServiceStatus> {
        let mut db = self.db()?;
        let tx = db.transaction()?;
        let prior: Option<String> = tx
            .query_row(
                "SELECT body FROM preferences WHERE key='emby-last-started-at'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let prior = prior
            .map(|s| serde_json::from_str::<String>(&s))
            .transpose()?;
        let previous = prior
            .as_deref()
            .map(chrono::DateTime::parse_from_rfc3339)
            .transpose()
            .map_err(|e| AppError::new("emby-start-time", e))?;
        if let Some(at) = status.last_started_at.as_ref() {
            let time = chrono::DateTime::parse_from_rfc3339(at)
                .map_err(|e| AppError::new("emby-start-time", e))?;
            if previous.is_none_or(|old| old < time) {
                self.writable()?;
                tx.execute("INSERT INTO preferences VALUES('emby-last-started-at',?1) ON CONFLICT(key) DO UPDATE SET body=excluded.body",[serde_json::to_string(at)?])?;
            } else {
                status.last_started_at = prior;
            }
        } else {
            status.last_started_at = prior;
        }
        tx.commit()?;
        Ok(status)
    }
}
