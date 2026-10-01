use super::*;
use crate::{legacy_process, windows_startup};
pub struct Windows<'a> {
    pub store: &'a Store,
}
impl Platform for Windows<'_> {
    fn inspect(&mut self) -> Result<Inventory> {
        Ok(crate::legacy_components::observe(self.store))
    }
    fn inspect_plan(&mut self, plan: &Plan) -> Result<Inventory> {
        let roots = crate::legacy_components::reviewed_roots(self.store, plan)?;
        Ok(crate::legacy_components::observe_with_roots(
            self.store, roots,
        ))
    }
    fn prepare_agent(&mut self, component: &Component) -> Result<Snapshot> {
        crate::legacy_agent::snapshot(std::path::Path::new(&component.target))
    }
    fn archive_agent(&mut self, value: &Snapshot, operation: &str) -> Result<()> {
        crate::legacy_agent::windows::archive(value, operation)
    }
    fn restore_agent(&mut self, value: &Snapshot, operation: &str) -> Result<()> {
        crate::legacy_agent::windows::restore(value, operation)
    }
    fn prepare_run(&mut self, component: &Component) -> Result<RunBackup> {
        windows_startup::prepare_legacy_run(&component.target)?.ok_or_else(|| {
            AppError::new(
                "legacy-components-review-changed",
                "旧登录项已改变，请重新检查",
            )
        })
    }
    fn stop_process(&mut self, component: &Component) -> Result<()> {
        let current = legacy_process::observe()?
            .into_iter()
            .find(|process| Some(process.pid) == component.pid)
            .ok_or_else(|| {
                AppError::new(
                    "legacy-process-review-changed",
                    "旧进程已退出或改变，请重新检查",
                )
            })?;
        legacy_process::verify_same_process(&current, &current)?;
        let image = current.executable.as_ref().expect("verified process image");
        let identity = crate::hash(&serde_json::to_vec(&(
            current.pid,
            &current.created,
            &image.sha256,
        ))?);
        if image.path != component.target || identity != component.identity {
            return Err(AppError::new(
                "legacy-process-review-changed",
                "旧进程身份与确认清单不一致",
            ));
        }
        legacy_process::request_stop(&current)
    }
    fn remove_run(&mut self, store: &Store, id: &str, backup: &str) -> Result<Removal> {
        match store.legacy_run_receipt(id) {
            Ok(receipt) => {
                if receipt.backup != backup {
                    return Err(AppError::new(
                        "legacy-startup-operation-conflict",
                        "登录项回执与迁移清单不一致",
                    ));
                }
                match receipt.phase {
                    RunPhase::Removed => Ok(Removal::Removed),
                    RunPhase::Absent => Ok(Removal::AlreadyAbsent),
                    _ => Err(AppError::new(
                        "legacy-startup-operation-unverified",
                        "登录项操作结果尚未确认，未再次删除",
                    )),
                }
            }
            Err(error) if error.code == "legacy-startup-operation-missing" => {
                windows_startup::remove_legacy_run(store, id, backup)
            }
            Err(error) => Err(error),
        }
    }
    fn restore_run(&mut self, store: &Store, id: &str) -> Result<()> {
        windows_startup::restore_legacy_run(store, id)
    }
}
