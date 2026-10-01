//! Fixed legacy task observation. A matching task name is not ownership.
use crate::{hash, AppError, Result};
use serde::Serialize;
pub const TASK_NAME: &str = "Emby Technical Specs Web Card";

#[derive(Debug, Serialize)]
pub struct TaskDefinition {
    pub xml_sha256: String,
    pub action_kinds: Vec<String>,
    pub action_sha256: Vec<String>,
}

/// Keep complete definition evidence without exporting command arguments that
/// could belong to another application's same-named task or contain secrets.
pub fn definition(xml: &str) -> Result<TaskDefinition> {
    if xml.len() > 1 << 20 {
        return Err(AppError::new("legacy-task-size", "旧计划任务定义过大"));
    }
    let document =
        roxmltree::Document::parse(xml).map_err(|e| AppError::new("legacy-task-xml", e))?;
    let root = document.root_element();
    let namespace = "http://schemas.microsoft.com/windows/2004/02/mit/task";
    if root.tag_name().name() != "Task" || root.tag_name().namespace() != Some(namespace) {
        return Err(AppError::new(
            "legacy-task-xml",
            "无法确认旧计划任务定义格式",
        ));
    }
    let containers: Vec<_> = root
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "Actions")
        .collect();
    if containers.len() != 1 || containers[0].tag_name().namespace() != Some(namespace) {
        return Err(AppError::new("legacy-task-xml", "旧计划任务动作清单有歧义"));
    }
    let actions: Vec<_> = containers[0]
        .children()
        .filter(|node| node.is_element())
        .collect();
    if actions.is_empty()
        || actions.len() > 32
        || actions
            .iter()
            .any(|node| node.tag_name().namespace() != Some(namespace))
    {
        return Err(AppError::new("legacy-task-xml", "旧计划任务动作清单无效"));
    }
    Ok(TaskDefinition {
        xml_sha256: hash(xml.as_bytes()),
        action_kinds: actions
            .iter()
            .map(|node| node.tag_name().name().to_owned())
            .collect(),
        action_sha256: actions
            .iter()
            .map(|node| hash(xml[node.range()].as_bytes()))
            .collect(),
    })
}

#[cfg(windows)]
#[derive(Debug, Serialize)]
pub struct TaskObservation {
    pub name: String,
    pub enabled: bool,
    pub state: i32,
    pub definition: TaskDefinition,
}

#[cfg(windows)]
pub fn observe() -> Result<Option<TaskObservation>> {
    use windows::{
        core::BSTR,
        Win32::System::{Com::*, TaskScheduler::*, Variant::VARIANT},
    };
    let fail = |error: windows::core::Error| AppError::new("legacy-task-read", error).at(TASK_NAME);
    struct Apartment(bool);
    impl Drop for Apartment {
        fn drop(&mut self) {
            if self.0 {
                unsafe {
                    CoUninitialize();
                }
            }
        }
    }
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    // A pre-existing apartment can use this synchronous interface. Balance only
    // this call's successful initialization, including S_FALSE.
    let _apartment = if initialized.is_ok() {
        Apartment(true)
    } else if initialized.0 == 0x80010106u32 as i32 {
        Apartment(false)
    } else {
        return Err(fail(initialized.into()));
    };
    unsafe {
        let service: ITaskService =
            CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER).map_err(fail)?;
        let empty = VARIANT::default();
        service
            .Connect(&empty, &empty, &empty, &empty)
            .map_err(fail)?;
        let folder = service.GetFolder(&BSTR::from("\\")).map_err(fail)?;
        let task = match folder.GetTask(&BSTR::from(TASK_NAME)) {
            Ok(task) => task,
            Err(error) if error.code().0 == 0x80070002u32 as i32 => return Ok(None),
            Err(error) => return Err(fail(error)),
        };
        let xml = task.Xml().map_err(fail)?;
        let text = String::from_utf16(&xml)
            .map_err(|e| AppError::new("legacy-task-encoding", e).at(TASK_NAME))?;
        Ok(Some(TaskObservation {
            name: TASK_NAME.into(),
            enabled: task.Enabled().map_err(fail)?.0 != 0,
            state: task.State().map_err(fail)?.0,
            definition: definition(&text)?,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn task_definition_binds_all_actions_without_disclosing_arguments() {
        let xml = r#"<Task xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task"><Actions><Exec><Command>C:\旧版\Manager.exe</Command><Arguments>--agent secret-example</Arguments></Exec><ComHandler><ClassId>other</ClassId></ComHandler></Actions></Task>"#;
        let value = definition(xml).unwrap();
        assert_eq!(value.action_kinds, ["Exec", "ComHandler"]);
        assert_eq!(value.action_sha256.len(), 2);
        let changed = definition(&xml.replace("secret-example", "changed-example")).unwrap();
        assert_ne!(value.xml_sha256, changed.xml_sha256);
        assert_ne!(value.action_sha256[0], changed.action_sha256[0]);
        assert_eq!(value.action_sha256[1], changed.action_sha256[1]);
        let exported = serde_json::to_string(&value).unwrap();
        assert!(!exported.contains("secret-example"));
        assert!(!exported.contains("Manager.exe"));
    }
    #[test]
    fn malformed_or_ambiguous_task_definitions_are_not_empty_tasks() {
        for body in [
            "<Actions/>",
            "<Actions/><Actions/>",
            "<Actions><Exec xmlns=\"other\"/></Actions>",
        ] {
            assert!(definition(&format!("<Task xmlns=\"http://schemas.microsoft.com/windows/2004/02/mit/task\">{body}</Task>")).is_err());
        }
        for xml in ["", "<Task>", "<Task><Actions><Exec/></Actions></Task>"] {
            assert!(definition(xml).is_err());
        }
    }
}
