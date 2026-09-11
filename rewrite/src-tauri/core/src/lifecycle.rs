use crate::{AppError, Result};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, Default)]
#[serde(rename_all = "kebab-case")]
pub enum CloseAction {
    #[default]
    Quit,
    Background,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
pub struct Settings {
    pub revision: u32,
    pub close_action: CloseAction,
    pub launch_at_login: bool,
    pub start_hidden: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            revision: 0,
            close_action: CloseAction::Quit,
            launch_at_login: false,
            start_hidden: true,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct SettingsOperation {
    pub id: String,
    pub phase: String,
    pub desired: Settings,
    pub previous: Settings,
    pub native_before: bool,
    pub error: Option<AppError>,
}
pub trait Autostart {
    fn enabled(&self) -> Result<bool>;
    fn set(&self, enabled: bool) -> Result<()>;
}
pub fn apply(
    store: &crate::store::Store,
    id: &str,
    desired: Settings,
    native: &impl Autostart,
) -> Result<SettingsOperation> {
    let operation = store.prepare_lifecycle(id, desired, native.enabled()?)?;
    if operation.phase == "committed" {
        return Ok(operation);
    }
    if operation.phase != "planned" {
        return Err(operation.error.unwrap_or_else(|| {
            AppError::new(
                "settings-operation-finished",
                "Create a new operation after inspecting the previous result",
            )
        }));
    }
    let result = (|| {
        if native.enabled()? != operation.desired.launch_at_login {
            native.set(operation.desired.launch_at_login)?;
        }
        if native.enabled()? != operation.desired.launch_at_login {
            return Err(AppError::new(
                "autostart-verification",
                "System did not retain the requested login setting",
            ));
        }
        store.finish_lifecycle(id, "committed", None)
    })();
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            let rollback = (|| {
                if native.enabled()? != operation.native_before {
                    native.set(operation.native_before)?;
                }
                if native.enabled()? != operation.native_before {
                    return Err(AppError::new(
                        "autostart-rollback",
                        "Login setting rollback could not be verified",
                    ));
                }
                Ok(())
            })();
            let failure = rollback.err().unwrap_or(error);
            let _ = store.finish_lifecycle(id, "failed", Some(failure.clone()));
            Err(failure)
        }
    }
}
pub fn reconcile(store: &crate::store::Store, native: &impl Autostart) -> Result<()> {
    let pending = store.preferences("lifecycle-pending")?;
    let Some(id) = pending.as_str() else {
        return Ok(());
    };
    let crate::OperationResult::Lifecycle(operation) = store.operation_result(id)? else {
        return Err(AppError::new(
            "settings-recovery",
            "Pending operation has the wrong kind",
        ));
    };
    if operation.phase == "planned" {
        if native.enabled()? == operation.desired.launch_at_login {
            store.finish_lifecycle(id, "committed", None)?;
        } else {
            store.finish_lifecycle(
                id,
                "interrupted",
                Some(AppError::new(
                    "settings-interrupted",
                    "Login setting was not applied before interruption; retry from settings",
                )),
            )?;
        }
    }
    Ok(())
}
