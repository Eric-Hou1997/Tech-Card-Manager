use super::*;
use crate::migration::MigrationReceipt;
use std::io::Read;

// windows/engine/windows-engine.ps1 at the scope.json v4.1.0 baseline. The
// PowerShell engine is read as provenance only and is never executed.
use crate::legacy_process::BASELINE_ENGINE;
fn destination_in_use(db: &Connection) -> Result<bool> {
    Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM configuration WHERE json_extract(body,'$.revision')>0) OR EXISTS(SELECT 1 FROM items) OR EXISTS(SELECT 1 FROM tasks) OR EXISTS(SELECT 1 FROM preferences WHERE key IN ('media-folders','incremental-settings','lifecycle-settings') OR (key='lifecycle-pending' AND body<>'null')) OR EXISTS(SELECT 1 FROM operations WHERE json_extract(result,'$.kind')='migration')", [], |row| row.get::<_,bool>(0))?)
}

fn fixed_file(root: &Path, relative: &str, limit: u64) -> Result<Vec<u8>> {
    let path = paths::within(root, &root.join(relative))?;
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .map_err(|e| AppError::new("migration-source-read", e).at(path.display()))?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| AppError::new("migration-source-read", e).at(path.display()))?;
    if bytes.len() as u64 > limit {
        return Err(AppError::new("migration-source-size", "旧版数据文件过大").at(path.display()));
    }
    Ok(bytes)
}

impl Store {
    /// Persisted source is a locator only; observers must recheck its files.
    /// Reading this receipt never imports data or touches the old directory.
    pub fn startup_import_receipt(&self) -> Result<Option<MigrationReceipt>> {
        let db = self.db()?;
        let completed: Option<String> = db.query_row(
            "SELECT result FROM operations WHERE id LIKE 'startup-tcm-%' AND json_extract(result,'$.kind')='migration' ORDER BY rowid LIMIT 1", [], |row| row.get(0),
        ).optional()?;
        match completed {
            Some(body) => match serde_json::from_str(&body)? {
                OperationResult::Migration(receipt) => Ok(Some(receipt)),
                _ => Err(AppError::new(
                    "migration-receipt-invalid",
                    "旧版导入回执格式无效",
                )),
            },
            None => Ok(None),
        }
    }
    /// Try the adjacent portable layout before consulting the fixed original
    /// login entry. A receipt or existing destination avoids any locator read.
    pub fn import_portable_from_startup_sources(
        &self,
        adjacent: &Path,
        locate: impl FnOnce() -> Result<Option<std::path::PathBuf>>,
    ) -> Result<Option<MigrationReceipt>> {
        let receipt = self.import_portable_on_start(adjacent)?;
        if receipt.is_some() || destination_in_use(&*self.db()?)? {
            return Ok(receipt);
        }
        let Some(source) = locate()? else {
            return Ok(None);
        };
        if source == adjacent {
            return Ok(None);
        }
        self.import_portable_on_start(&source)
    }

    /// Reads a specifically located original portable workspace. A durable
    /// receipt wins before touching the old source, including after its removal.
    pub fn import_portable_on_start(&self, portable: &Path) -> Result<Option<MigrationReceipt>> {
        if let Some(receipt) = self.startup_import_receipt()? {
            return Ok(Some(receipt));
        }
        {
            let db = self.db()?;
            // Never merge old choices automatically into an already used app.
            if destination_in_use(&db)? {
                return Ok(None);
            }
        }
        match std::fs::symlink_metadata(portable.join("data/settings.json")) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(AppError::new("migration-source-read", error)
                    .at(portable.join("data/settings.json").display()))
            }
            Ok(_) => {}
        }
        let source = paths::checked(portable)?;
        let engine = fixed_file(&source, "runtime/engine/windows-engine.ps1", 8 << 20)?;
        if hash(&engine) != BASELINE_ENGINE {
            return Err(AppError::new(
                "migration-source-unverified",
                "无法确认旧版 Card 软件数据来源",
            )
            .at(source.display()));
        }
        let settings = fixed_file(&source, "data/settings.json", 1 << 20)?;
        let value: serde_json::Value = serde_json::from_slice(
            settings
                .strip_prefix(&[0xef, 0xbb, 0xbf])
                .unwrap_or(&settings),
        )
        .map_err(|e| {
            AppError::new("migration-settings-invalid", e)
                .at(source.join("data/settings.json").display())
        })?;
        if !value.is_object() {
            return Err(
                AppError::new("migration-settings-invalid", "旧版设置格式无效")
                    .at(source.join("data/settings.json").display()),
            );
        }
        let current = self.configuration()?;
        let snapshot = crate::migration::prepare("startup", &source, "tcm-portable", &current)?;
        // Changed files after an interrupted attempt receive a fresh immutable
        // plan. A completed receipt above prevents any repeat import.
        let id = format!("startup-tcm-{}", snapshot.fingerprint);
        let plan = self.prepare_migration(&id, &source, "tcm-portable")?;
        if serde_json::to_vec(&snapshot.files)? != serde_json::to_vec(&plan.files)?
            || current.revision != plan.configuration_revision
        {
            return Err(
                AppError::new("migration-source-changed", "旧版数据已改变，请重试")
                    .at(source.display()),
            );
        }
        self.apply_migration(&id, &plan.fingerprint).map(Some)
    }
}
