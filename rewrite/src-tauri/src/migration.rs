use crate::desktop::Desktop;
use product_core::{
    migration::{MigrationPlan, MigrationReceipt},
    *,
};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
#[tauri::command]
pub async fn migration_plan(
    id: String,
    source_kind: String,
    app: tauri::AppHandle,
) -> Result<Option<MigrationPlan>> {
    let allowed = if crate::PRODUCT == "ITM" {
        ["itm-manager", "itm-engine"]
    } else {
        ["tcm-portable", "tcm-state"]
    };
    if !allowed.contains(&source_kind.as_str()) {
        return Err(AppError::new(
            "migration-product",
            "Source kind belongs to another product",
        ));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let Some(folder) = app
            .dialog()
            .file()
            .set_title("选择旧版数据目录 / Select legacy data directory")
            .blocking_pick_folder()
        else {
            return Ok(None);
        };
        let path = folder
            .into_path()
            .map_err(|e| AppError::new("invalid-path", e))?;
        app.state::<Desktop>()
            .store
            .prepare_migration(&id, &path, &source_kind)
            .map(Some)
    })
    .await
    .map_err(|e| AppError::new("migration-worker", e))?
}
#[tauri::command]
pub async fn migration_apply(
    id: String,
    fingerprint: String,
    app: tauri::AppHandle,
) -> Result<MigrationReceipt> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<Desktop>()
            .store
            .apply_migration(&id, &fingerprint)
    })
    .await
    .map_err(|e| AppError::new("migration-worker", e))?
}
#[tauri::command]
pub fn migration_result(id: String, state: State<'_, Desktop>) -> Result<OperationResult> {
    state.store.operation_result(&id)
}
