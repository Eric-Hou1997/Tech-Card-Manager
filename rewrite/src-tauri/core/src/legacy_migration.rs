//! Confirmed system migration, with a durable step journal. The native session
//! must serialize prepare/execute/recovery; this module never launches a program.
use crate::{
    legacy_agent::Snapshot,
    legacy_components::{Component, Inventory},
    legacy_startup::{Removal, RunBackup, RunPhase},
    store::Store,
    AppError, Result,
};
use serde::{Deserialize, Serialize};
#[cfg(windows)]
pub mod windows;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Planned,
    Running,
    Complete,
    Failed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepPhase {
    Pending,
    Working,
    Done,
    Absent,
    Restored,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub component: Component,
    pub backup: Option<String>,
    pub operation: String,
    pub phase: StepPhase,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub id: String,
    pub reviewed: String,
    pub inventory: Inventory,
    pub steps: Vec<Step>,
    pub phase: Phase,
    pub error: Option<AppError>,
    pub recovery_errors: Vec<AppError>,
}
impl Plan {
    pub fn validate(&self) -> Result<()> {
        if self.reviewed != self.inventory.fingerprint()?
            || self.steps.len() != self.inventory.items.len()
            || self
                .steps
                .iter()
                .zip(&self.inventory.items)
                .enumerate()
                .any(|(index, (step, item))| {
                    step.component != *item
                        || step.operation != step_id(&self.id, index)
                        || matches!(item.kind.as_str(), "startup" | "agent-file")
                            != step.backup.is_some()
                })
        {
            return Err(AppError::new(
                "legacy-system-plan-conflict",
                "旧组件迁移回执与确认清单不一致",
            ));
        }
        supported(&self.inventory)
    }
}
fn step_id(id: &str, index: usize) -> String {
    crate::hash(format!("legacy-system:{id}:{index}").as_bytes())
}
fn verify_backup(value: &RunBackup, component: &Component) -> Result<()> {
    value.path()?;
    let command = crate::hash(
        &value
            .command
            .iter()
            .flat_map(|unit| unit.to_le_bytes())
            .collect::<Vec<_>>(),
    );
    let identity = crate::hash(&serde_json::to_vec(&(
        Some(command),
        Some(&value.executable),
    ))?);
    if value.name != component.target || identity != component.identity {
        return Err(AppError::new(
            "legacy-components-review-changed",
            "旧登录项与确认清单不一致",
        ));
    }
    Ok(())
}
fn verify_agent(value: &Snapshot, component: &Component) -> Result<()> {
    if value.path != component.target || value.fingerprint()? != component.identity {
        return Err(AppError::new(
            "legacy-components-review-changed",
            "旧状态文件与确认清单不一致",
        )
        .at(&component.target));
    }
    Ok(())
}
fn supported(inventory: &Inventory) -> Result<()> {
    if let Some(error) = inventory.errors.first() {
        return Err(error.clone());
    }
    for item in &inventory.items {
        if !matches!(item.kind.as_str(), "process" | "startup" | "agent-file") {
            return Err(AppError::new(
                "legacy-system-component-unverified",
                "该旧组件尚不能安全迁移，未开始清理",
            )
            .at(&item.target));
        }
    }
    Ok(())
}

pub trait Platform {
    fn inspect(&mut self) -> Result<Inventory>;
    fn inspect_plan(&mut self, _plan: &Plan) -> Result<Inventory> {
        self.inspect()
    }
    fn prepare_run(&mut self, component: &Component) -> Result<RunBackup>;
    fn stop_process(&mut self, component: &Component) -> Result<()>;
    fn prepare_agent(&mut self, component: &Component) -> Result<Snapshot> {
        Err(AppError::new(
            "legacy-system-component-unverified",
            "此平台尚不能安全迁移旧状态文件",
        )
        .at(&component.target))
    }
    fn archive_agent(&mut self, _value: &Snapshot, _operation: &str) -> Result<()> {
        Err(AppError::new(
            "legacy-system-component-unverified",
            "此平台尚不能安全迁移旧状态文件",
        ))
    }
    fn restore_agent(&mut self, _value: &Snapshot, _operation: &str) -> Result<()> {
        Err(AppError::new(
            "legacy-system-component-unverified",
            "此平台尚不能安全恢复旧状态文件",
        ))
    }
    /// Must recover the same durable Run operation, never delete twice on reply loss.
    fn remove_run(&mut self, store: &Store, id: &str, backup: &str) -> Result<Removal>;
    fn restore_run(&mut self, store: &Store, id: &str) -> Result<()>;
}

pub fn prepare(
    store: &Store,
    platform: &mut impl Platform,
    id: &str,
    reviewed: &str,
) -> Result<Plan> {
    if let Some(plan) = store.legacy_system_plan(id)? {
        if plan.reviewed != reviewed {
            return Err(AppError::new(
                "legacy-system-plan-conflict",
                "迁移编号已用于另一份确认清单",
            ));
        }
        return Ok(plan);
    }
    let inventory = platform.inspect()?;
    if inventory.fingerprint()? != reviewed {
        return Err(AppError::new(
            "legacy-components-review-changed",
            "旧版组件已改变，请重新确认迁移清单",
        ));
    }
    supported(&inventory)?; // Check every unsupported/unknown item before any side effect.
    let mut steps = Vec::new();
    for (index, component) in inventory.items.iter().enumerate() {
        let backup = if component.kind == "startup" {
            let value = platform.prepare_run(component)?;
            verify_backup(&value, component)?;
            Some(store.backup_legacy_run(&value)?)
        } else if component.kind == "agent-file" {
            let value = platform.prepare_agent(component)?;
            verify_agent(&value, component)?;
            Some(store.backup_legacy_agent(&value)?)
        } else {
            None
        };
        steps.push(Step {
            component: component.clone(),
            backup,
            operation: step_id(id, index),
            phase: StepPhase::Pending,
        });
    }
    if platform.inspect()?.fingerprint()? != reviewed {
        return Err(AppError::new(
            "legacy-components-review-changed",
            "旧版组件在准备期间改变，未开始清理",
        ));
    }
    let plan = Plan {
        id: id.into(),
        reviewed: reviewed.into(),
        inventory,
        steps,
        phase: Phase::Planned,
        error: None,
        recovery_errors: vec![],
    };
    store.save_legacy_system_plan(None, &plan)?;
    Ok(plan)
}
fn save(store: &Store, plan: &mut Plan, update: impl FnOnce(&mut Plan)) -> Result<()> {
    let mut next = plan.clone();
    update(&mut next);
    store.save_legacy_system_plan(Some(plan), &next)?;
    *plan = next;
    Ok(())
}
fn remaining(plan: &Plan, current: &Inventory) -> Result<()> {
    if let Some(error) = current.errors.first() {
        return Err(error.clone());
    }
    for item in &current.items {
        if !plan.steps.iter().any(|step| {
            matches!(step.phase, StepPhase::Pending | StepPhase::Working) && step.component == *item
        }) {
            return Err(AppError::new(
                "legacy-components-review-changed",
                "发现新出现或已改变的旧组件，已停止迁移",
            )
            .at(&item.target));
        }
    }
    Ok(())
}
pub fn execute(
    store: &Store,
    platform: &mut impl Platform,
    id: &str,
    reviewed: &str,
) -> Result<Plan> {
    let mut plan = prepare(store, platform, id, reviewed)?;
    if plan.phase == Phase::Failed {
        return Err(plan.error.clone().unwrap_or_else(|| {
            AppError::new("legacy-system-failed", "旧组件迁移未完成，请重新检查")
        }));
    }
    // A restarted plan must still have every verified backup before stopping
    // even the first process. A journal entry is not proof of intact backup bytes.
    for step in &plan.steps {
        if let Some(backup) = &step.backup {
            if step.component.kind == "agent-file" {
                verify_agent(&store.legacy_agent_backup(backup)?, &step.component)?;
            } else {
                verify_backup(&store.legacy_run_backup(backup)?, &step.component)?;
            }
        }
    }
    if plan.phase == Phase::Complete {
        platform.inspect_plan(&plan)?.require_clear()?;
        return Ok(plan);
    }
    save(store, &mut plan, |p| p.phase = Phase::Running)?;
    let result = (|| -> Result<()> {
        // Original order: normal process shutdown before disabling old login entries.
        let mut order: Vec<_> = (0..plan.steps.len()).collect();
        order.sort_by_key(|index| match plan.steps[*index].component.kind.as_str() {
            "process" => 0,
            "startup" => 1,
            _ => 2,
        });
        for index in order {
            if matches!(plan.steps[index].phase, StepPhase::Done | StepPhase::Absent) {
                continue;
            }
            let current = platform.inspect_plan(&plan)?;
            remaining(&plan, &current)?;
            // Normal original-process shutdown may itself remove these files.
            // Only an unstarted step may record that independent absence.
            if plan.steps[index].component.kind == "agent-file"
                && plan.steps[index].phase == StepPhase::Pending
                && !current.items.contains(&plan.steps[index].component)
            {
                save(store, &mut plan, |p| {
                    p.steps[index].phase = StepPhase::Absent
                })?;
                continue;
            }
            save(store, &mut plan, |p| {
                p.steps[index].phase = StepPhase::Working
            })?;
            let step = &plan.steps[index];
            let outcome = if step.component.kind == "process" {
                if current.items.contains(&step.component) {
                    platform.stop_process(&step.component)?;
                    StepPhase::Done
                } else {
                    StepPhase::Absent
                }
            } else if step.component.kind == "agent-file" {
                let value = store
                    .legacy_agent_backup(step.backup.as_deref().expect("validated Agent backup"))?;
                platform.archive_agent(&value, &step.operation)?;
                StepPhase::Done
            } else {
                match platform.remove_run(
                    store,
                    &step.operation,
                    step.backup.as_deref().expect("validated startup backup"),
                )? {
                    Removal::Removed => StepPhase::Done,
                    Removal::AlreadyAbsent => StepPhase::Absent,
                }
            };
            // The native adapter verifies its own mutation. Recheck the whole
            // inventory as well; a still-live or newly replaced item is a failure.
            let after = platform.inspect_plan(&plan)?;
            if after.items.contains(&step.component) {
                return Err(
                    AppError::new("legacy-system-verification", "旧组件处理后仍然存在")
                        .at(&step.component.target),
                );
            }
            remaining(&plan, &after)?;
            save(store, &mut plan, |p| p.steps[index].phase = outcome)?;
        }
        platform.inspect_plan(&plan)?.require_clear()?;
        save(store, &mut plan, |p| p.phase = Phase::Complete)
    })();
    if let Err(error) = result {
        // Do not relaunch stopped processes. Restore only a Run value whose own
        // durable receipt proves this migration removed it; unknown stays unknown.
        let mut recovery_errors = Vec::new();
        for index in (0..plan.steps.len()).rev() {
            let step = &plan.steps[index];
            if step.backup.is_none() || step.phase == StepPhase::Pending {
                continue;
            }
            if step.component.kind == "agent-file" {
                if step.phase == StepPhase::Absent {
                    continue;
                }
                let result = store
                    .legacy_agent_backup(step.backup.as_deref().expect("validated Agent backup"))
                    .and_then(|value| platform.restore_agent(&value, &step.operation));
                match result {
                    Err(failure) => recovery_errors.push(failure),
                    Ok(()) => {
                        if let Err(failure) = save(store, &mut plan, |p| {
                            p.steps[index].phase = StepPhase::Restored
                        }) {
                            recovery_errors.push(failure);
                        }
                    }
                }
                continue;
            }
            match store.legacy_run_receipt(&step.operation) {
                Ok(receipt) if matches!(receipt.phase, RunPhase::Removed | RunPhase::Restoring) => {
                    if let Err(failure) = platform.restore_run(store, &step.operation) {
                        recovery_errors.push(failure);
                    } else if let Err(failure) = save(store, &mut plan, |p| {
                        p.steps[index].phase = StepPhase::Restored
                    }) {
                        recovery_errors.push(failure);
                    }
                }
                Ok(receipt) if receipt.phase == RunPhase::Removing => recovery_errors.push(
                    AppError::new(
                        "legacy-startup-restore-unverified",
                        "登录项删除结果不明，未自动恢复",
                    )
                    .at(&step.component.target),
                ),
                Ok(_) => {}
                Err(failure) => recovery_errors.push(failure),
            }
        }
        save(store, &mut plan, |p| {
            p.phase = Phase::Failed;
            p.error = Some(error.clone());
            p.recovery_errors = recovery_errors;
        })?;
        return Err(error);
    }
    Ok(plan)
}
