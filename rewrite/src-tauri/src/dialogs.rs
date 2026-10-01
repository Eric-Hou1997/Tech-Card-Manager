use product_core::{AppError, Result};
use serde::Deserialize;
use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    SaveRoots,
    RepairWeb,
    RebuildIndex,
    DisableIntegration,
}
fn message(action: Action, platform: &str) -> String {
    match action {
        Action::SaveRoots => format!("保存后，{platform} 只会索引启用的媒体目录。媒体 NFO 始终只读，不会被修改。继续？"),
        Action::RepairWeb => "这会先验证 Emby index.html、建立外部备份，再事务化维护网页卡片。无法确认安全时会直接停止。继续？".into(),
        Action::RebuildIndex => "这会重新解析已配置范围内的全部 NFO。媒体 NFO 和 Emby index.html 都不会被修改。继续？".into(),
        Action::DisableIntegration => "这会停止新版服务，并事务化移除本程序拥有的网页卡片集成。媒体 NFO 不会被修改。\n\n确定恢复原生 Emby？".into(),
    }
}
#[tauri::command]
pub async fn confirm_product_action(action: Action, window: tauri::WebviewWindow) -> Result<bool> {
    let platform = match std::env::consts::OS {
        "windows" => "Windows",
        "macos" => "macOS",
        _ => "Linux",
    };
    let app = window.app_handle();
    let snapshot = crate::languages::snapshot(app, &app.state::<crate::languages::Languages>())?;
    // Resolve the original Windows message before substituting the host name,
    // so the pinned language-pack message ID remains the original one.
    let text = crate::languages::web_message(&snapshot, &message(action, "Windows"))?
        .replace("Windows", platform);
    let dialog = window
        .app_handle()
        .dialog()
        .message(text)
        .title("Tech Card Manager")
        .buttons(MessageDialogButtons::OkCancel)
        .parent(&window);
    tauri::async_runtime::spawn_blocking(move || dialog.blocking_show())
        .await
        .map_err(|error| AppError::new("confirmation-dialog", error))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confirmation_texts_come_from_the_original_product_actions() {
        let source = include_str!("../../../windows/web/index.html");
        for action in [
            Action::SaveRoots,
            Action::RepairWeb,
            Action::RebuildIndex,
            Action::DisableIntegration,
        ] {
            assert!(source.contains(&message(action, "Windows").replace('\n', "\\n")));
        }
        for invalid in ["delete", "arbitrary-message", "open", "shell"] {
            assert!(serde_json::from_value::<Action>(serde_json::json!(invalid)).is_err());
        }
    }
}
