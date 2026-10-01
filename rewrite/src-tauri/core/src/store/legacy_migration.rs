use super::*;
use crate::legacy_migration::Plan;
impl Store {
    pub fn latest_legacy_system_plan(&self) -> Result<Option<Plan>> {
        let id: Option<String> = self
            .db()?
            .query_row(
                "SELECT body FROM preferences WHERE key='legacy-system-last'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        id.map(|id| {
            let id: String = serde_json::from_str(&id)?;
            self.legacy_system_plan(&id)?
                .ok_or_else(|| AppError::new("legacy-system-plan-missing", "旧组件迁移记录缺失"))
        })
        .transpose()
    }
    pub fn legacy_system_plan(&self, id: &str) -> Result<Option<Plan>> {
        valid_id(id)?;
        let body: Option<String> = self
            .db()?
            .query_row(
                "SELECT body FROM preferences WHERE key=?1",
                [format!("legacy-system:{id}")],
                |row| row.get(0),
            )
            .optional()?;
        body.map(|body| {
            let plan: Plan = serde_json::from_str(&body)?;
            if plan.id != id {
                return Err(AppError::new(
                    "legacy-system-plan-conflict",
                    "旧组件迁移编号不匹配",
                ));
            }
            plan.validate()?;
            Ok(plan)
        })
        .transpose()
    }
    pub(crate) fn save_legacy_system_plan(
        &self,
        expected: Option<&Plan>,
        next: &Plan,
    ) -> Result<()> {
        valid_id(&next.id)?;
        next.validate()?;
        let mut db = self.db()?;
        self.writable()?;
        let tx = db.transaction()?;
        let key = format!("legacy-system:{}", next.id);
        let body = serde_json::to_string(next)?;
        let changed = if let Some(expected) = expected {
            if expected.id != next.id || expected.reviewed != next.reviewed {
                return Err(AppError::new(
                    "legacy-system-plan-conflict",
                    "旧组件确认清单不能改绑",
                ));
            }
            tx.execute(
                "UPDATE preferences SET body=?1 WHERE key=?2 AND body=?3",
                params![body, key, serde_json::to_string(expected)?],
            )?
        } else {
            let last: Option<String> = tx
                .query_row(
                    "SELECT body FROM preferences WHERE key='legacy-system-last'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(last) = last {
                let last: String = serde_json::from_str(&last)?;
                let previous: String = tx.query_row(
                    "SELECT body FROM preferences WHERE key=?1",
                    [format!("legacy-system:{last}")],
                    |row| row.get(0),
                )?;
                let previous: Plan = serde_json::from_str(&previous)?;
                previous.validate()?;
                if matches!(
                    previous.phase,
                    crate::legacy_migration::Phase::Planned
                        | crate::legacy_migration::Phase::Running
                ) {
                    return Err(AppError::new(
                        "legacy-system-recovery-required",
                        "请先恢复上一次旧组件迁移",
                    ));
                }
            }
            tx.execute(
                "INSERT INTO preferences(key,body) VALUES(?1,?2) ON CONFLICT(key) DO NOTHING",
                params![key, body],
            )?
        };
        if changed != 1 {
            return Err(AppError::new(
                "legacy-system-plan-conflict",
                "旧组件迁移回执已改变，请重新检查",
            ));
        }
        if expected.is_none() {
            tx.execute("INSERT INTO preferences(key,body) VALUES('legacy-system-last',?1) ON CONFLICT(key) DO UPDATE SET body=excluded.body",[serde_json::to_string(&next.id)?])?;
        }
        tx.commit()?;
        Ok(())
    }
}
