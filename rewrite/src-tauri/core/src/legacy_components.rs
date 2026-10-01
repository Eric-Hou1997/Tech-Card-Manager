//! Pre-start observations, not authorization to terminate or remove anything.
use crate::{hash, AppError, Result};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(rename = "LegacyComponent")]
pub struct Component {
    pub kind: String,
    pub target: String,
    pub identity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}
#[derive(Debug, Clone, Serialize, TS)]
#[ts(rename = "LegacyComponentsReview")]
pub struct Review {
    pub fingerprint: String,
    pub items: Vec<String>,
    pub errors: Vec<AppError>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Inventory {
    pub items: Vec<Component>,
    pub errors: Vec<AppError>,
}
impl Inventory {
    pub fn review(&self) -> Result<Review> {
        Ok(Review {
            fingerprint: self.fingerprint()?,
            items: self
                .items
                .iter()
                .map(|item| match item.kind.as_str() {
                    "process" => match item.pid {
                        Some(pid) => format!("旧版程序 PID {pid}：{}", item.target),
                        None => format!("旧版程序：{}", item.target),
                    },
                    "startup" => format!("旧版登录启动项：{}", item.target),
                    "task" => format!("旧版计划任务: {}", item.target),
                    "agent-file" => format!("旧版 Agent 状态文件：{}", item.target),
                    _ => format!("无法确认的旧组件：{}", item.target),
                })
                .collect(),
            errors: self.errors.clone(),
        })
    }
    pub fn fingerprint(&self) -> Result<String> {
        Ok(hash(&serde_json::to_vec(self)?))
    }
    pub fn require_reviewed_clear(&self, reviewed: &str) -> Result<()> {
        if reviewed != self.fingerprint()? {
            return Err(AppError::new(
                "legacy-components-review-changed",
                "旧版组件已改变，请重新确认迁移清单",
            ));
        }
        self.require_clear()
    }
    pub fn require_clear(&self) -> Result<()> {
        if let Some(error) = self.errors.first() {
            return Err(error.clone());
        }
        if let Some(item) = self.items.first() {
            return Err(AppError::new(
                "legacy-components-require-plan",
                "检测到旧版组件，需要用户确认迁移",
            )
            .at(&item.target));
        }
        Ok(())
    }
}

pub fn start_after_check<T>(
    observe: impl FnOnce() -> Result<Inventory>,
    start: impl FnOnce() -> Result<T>,
) -> Result<T> {
    observe()?.require_clear()?;
    start()
}

#[cfg(windows)]
pub fn observe(store: &crate::store::Store) -> Inventory {
    let roots = store
        .latest_legacy_system_plan()
        .and_then(|plan| match plan {
            Some(plan) => reviewed_roots(store, &plan),
            None => Ok(vec![]),
        });
    match roots {
        Ok(roots) => observe_with_roots(store, roots),
        Err(error) => Inventory {
            items: vec![],
            errors: vec![error],
        },
    }
}
pub fn reviewed_roots(
    store: &crate::store::Store,
    plan: &crate::legacy_migration::Plan,
) -> Result<Vec<std::path::PathBuf>> {
    use std::path::{Path, PathBuf};
    let mut roots = Vec::new();
    for step in &plan.steps {
        let path = match step.component.kind.as_str() {
            "agent-file" => {
                crate::legacy_agent::fixed_path(Path::new(&step.component.target))?;
                Path::new(&step.component.target)
                    .parent()
                    .and_then(Path::parent)
                    .map(Path::to_path_buf)
            }
            "process" => Path::new(&step.component.target)
                .parent()
                .map(Path::to_path_buf),
            "startup" => {
                let backup =
                    store.legacy_run_backup(step.backup.as_deref().ok_or_else(|| {
                        AppError::new("legacy-startup-backup-missing", "旧登录项备份不存在")
                    })?)?;
                PathBuf::from(backup.executable.path)
                    .parent()
                    .map(Path::to_path_buf)
            }
            _ => None,
        };
        if let Some(root) = path {
            roots.push(root);
        }
    }
    Ok(roots)
}
#[cfg(windows)]
pub fn observe_with_roots(
    store: &crate::store::Store,
    known: Vec<std::path::PathBuf>,
) -> Inventory {
    use crate::{legacy_process, legacy_task, windows_startup};
    use std::{
        collections::BTreeSet,
        path::{Path, PathBuf},
    };
    let mut inventory = Inventory::default();
    let mut roots: BTreeSet<_> = known.into_iter().collect();
    match store.startup_import_receipt() {
        Ok(Some(receipt)) => {
            roots.insert(PathBuf::from(receipt.source));
        }
        Ok(None) => {}
        Err(error) => inventory.errors.push(error),
    }
    match legacy_process::observe() {
        Ok(processes) => {
            for process in processes {
                if let Some(error) = process.error {
                    inventory.errors.push(error);
                }
                if let Some(executable) = process.executable {
                    // A candidate without baseline provenance stays unclaimed.
                    // Even a match only blocks concurrent startup; it never grants
                    // termination rights or proves the loaded image's identity.
                    if executable.baseline_assets_match {
                        if let Some(root) = Path::new(&executable.path).parent() {
                            roots.insert(root.to_path_buf());
                        }
                        inventory.items.push(Component {
                            pid: Some(process.pid),
                            kind: "process".into(),
                            target: executable.path,
                            identity: hash(
                                &serde_json::to_vec(&(
                                    process.pid,
                                    process.created,
                                    executable.sha256,
                                ))
                                .expect("primitive serialization"),
                            ),
                        });
                    }
                }
            }
        }
        Err(error) => inventory.errors.push(error),
    }
    match windows_startup::legacy_entries() {
        Ok(entries) => {
            for entry in entries {
                if let Some(error) = entry.error {
                    inventory.errors.push(error);
                }
                if entry.present == Some(true) && !entry.current_program {
                    if let Some(executable) = &entry.executable {
                        if let Some(root) = Path::new(executable).parent() {
                            roots.insert(root.to_path_buf());
                        }
                    }
                    inventory.items.push(Component {
                        pid: None,
                        kind: "startup".into(),
                        target: entry.name,
                        identity: hash(
                            &serde_json::to_vec(&(entry.command_sha256, entry.source))
                                .expect("primitive serialization"),
                        ),
                    });
                }
            }
        }
        Err(error) => inventory.errors.push(error),
    }
    match legacy_task::observe() {
        Ok(Some(task)) => inventory.items.push(Component {
            pid: None,
            kind: "task".into(),
            target: task.name,
            identity: task.definition.xml_sha256,
        }),
        Ok(None) => {}
        Err(error) => inventory.errors.push(error),
    }
    for root in roots {
        match agent_files_at_known_source(&root) {
            Ok(files) => {
                for file in files {
                    if let Some(error) = file.error {
                        inventory.errors.push(error);
                    }
                    inventory.items.push(Component {
                        pid: None,
                        kind: "agent-file".into(),
                        target: file.path,
                        identity: file.identity,
                    });
                }
            }
            Err(error) => inventory.errors.push(error),
        }
    }
    inventory
        .items
        .sort_by(|a, b| (&a.kind, &a.target, &a.identity).cmp(&(&b.kind, &b.target, &b.identity)));
    inventory
}

/// Only a known, reachable local source can prove its fixed files absent. An
/// offline drive or unreadable ancestor remains unknown, not a clean report.
pub fn agent_files_at_known_source(
    root: &std::path::Path,
) -> Result<Vec<crate::legacy_process::AgentFile>> {
    if !root.is_absolute()
        || root
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(
            AppError::new("invalid-path", "Unambiguous absolute path required").at(root.display()),
        );
    }
    let mut present = false;
    for name in ["agent.pid", "agent-heartbeat.txt"] {
        present |= known_path_exists(&root.join("data").join(name))?;
    }
    if present {
        crate::legacy_process::agent_files(root)
    } else {
        Ok(Vec::new())
    }
}
fn known_path_exists(path: &std::path::Path) -> Result<bool> {
    let mut ancestor = path;
    loop {
        match std::fs::symlink_metadata(ancestor) {
            Ok(_) => {
                crate::paths::checked(ancestor)?;
                return Ok(ancestor == path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ancestor = ancestor.parent().ok_or_else(|| {
                    AppError::new("legacy-agent-source", error).at(path.display())
                })?;
            }
            Err(error) => {
                return Err(AppError::new("legacy-agent-source", error).at(path.display()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn review_keeps_itemized_original_process_identity_and_unknown_errors() {
        let mut inventory = Inventory {
            items: vec![Component {
                kind: "process".into(),
                target: "C:\\旧版\\Manager.exe".into(),
                identity: "image-and-creation-time".into(),
                pid: Some(123),
            }],
            errors: vec![AppError::new("read-denied", "读取失败").at("C:\\旧目录")],
        };
        let review = inventory.review().unwrap();
        assert_eq!(review.items, ["旧版程序 PID 123：C:\\旧版\\Manager.exe"]);
        assert_eq!(review.errors[0].code, "read-denied");
        assert_eq!(review.fingerprint, inventory.fingerprint().unwrap());
        assert_eq!(
            inventory
                .require_reviewed_clear(&review.fingerprint)
                .unwrap_err()
                .code,
            "read-denied"
        );
        let public = serde_json::to_string(&review).unwrap();
        assert!(!public.contains("image-and-creation-time"));
        inventory.items[0].pid = Some(124);
        assert_eq!(
            inventory
                .require_reviewed_clear(&review.fingerprint)
                .unwrap_err()
                .code,
            "legacy-components-review-changed"
        );
        assert_ne!(inventory.review().unwrap().fingerprint, review.fingerprint);
        inventory.items.clear();
        assert!(inventory.require_clear().is_err());
        assert_eq!(inventory.review().unwrap().errors.len(), 1);
        let clear = Inventory::default();
        assert!(clear
            .require_reviewed_clear(&clear.fingerprint().unwrap())
            .is_ok());
        assert!(clear.require_reviewed_clear(&review.fingerprint).is_err());
    }
    #[test]
    fn only_a_successful_empty_inventory_allows_start() {
        assert!(Inventory::default().require_clear().is_ok());
        for kind in ["process", "startup", "task", "agent-file"] {
            let report = Inventory {
                items: vec![Component {
                    pid: None,
                    kind: kind.into(),
                    target: "old target".into(),
                    identity: "v1".into(),
                }],
                errors: vec![],
            };
            assert_eq!(
                report.require_clear().unwrap_err().code,
                "legacy-components-require-plan"
            );
            let mut changed = report.clone();
            changed.items[0].identity = "v2".into();
            assert_ne!(
                report.fingerprint().unwrap(),
                changed.fingerprint().unwrap()
            );
        }
        let denied = Inventory {
            errors: vec![AppError::new("read-denied", "unverified")],
            ..Default::default()
        };
        assert_eq!(denied.require_clear().unwrap_err().code, "read-denied");
    }

    #[test]
    fn failed_or_nonempty_observations_never_invoke_the_start_action() {
        for observation in [
            Err(AppError::new("worker-failed", "unknown")),
            Ok(Inventory {
                errors: vec![AppError::new("read-denied", "unknown")],
                ..Default::default()
            }),
            Ok(Inventory {
                items: vec![Component {
                    pid: None,
                    kind: "task".into(),
                    target: "old task".into(),
                    identity: "definition".into(),
                }],
                ..Default::default()
            }),
        ] {
            assert!(start_after_check(
                || observation,
                || -> Result<()> { panic!("must not start") }
            )
            .is_err());
        }
        assert_eq!(
            start_after_check(|| Ok(Inventory::default()), || Ok(42)).unwrap(),
            42
        );
        assert_eq!(
            start_after_check(
                || Ok(Inventory::default()),
                || Err::<(), _>(AppError::new("start-failed", "failed"))
            )
            .unwrap_err()
            .code,
            "start-failed"
        );
    }
    #[test]
    fn removed_source_is_absent_but_unknown_existing_source_is_not_clear() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        assert!(agent_files_at_known_source(&root.join("removed/data"))
            .unwrap()
            .is_empty());
        assert!(agent_files_at_known_source(&root).unwrap().is_empty());
        std::fs::create_dir(root.join("data")).unwrap();
        std::fs::write(root.join("data/agent.pid"), b"123").unwrap();
        assert!(agent_files_at_known_source(&root).is_err());
        assert!(agent_files_at_known_source(std::path::Path::new("relative")).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("missing"), root.join("link")).unwrap();
            assert!(agent_files_at_known_source(&root.join("link/data")).is_err());
        }
    }
}
