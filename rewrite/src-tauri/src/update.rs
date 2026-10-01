#[cfg(target_os = "linux")]
use product_core::AppError;
use product_core::{update::InstallationIdentity, Result};
#[cfg(target_os = "linux")]
fn error(code: &str, e: impl ToString) -> AppError {
    AppError::new(code, e)
}
fn identity(app: &tauri::AppHandle) -> Result<InstallationIdentity> {
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else {
        std::env::consts::OS
    };
    #[cfg(target_os = "linux")]
    let channel = {
        if app.env().appimage.is_some() {
            "appimage".to_string()
        } else {
            let exe = std::env::current_exe().map_err(|e| error("update-installation-path", e))?;
            if std::process::Command::new("dpkg-query")
                .arg("--search")
                .arg(&exe)
                .output()
                .is_ok_and(|o| o.status.success())
            {
                "deb".into()
            } else if std::process::Command::new("rpm")
                .arg("-qf")
                .arg(&exe)
                .output()
                .is_ok_and(|o| o.status.success())
            {
                "rpm".into()
            } else {
                return Err(error(
                    "update-installation-channel",
                    "Executable is not owned by a recognized installation channel",
                ));
            }
        }
    };
    #[cfg(not(target_os = "linux"))]
    let channel = {
        let _ = app;
        if cfg!(windows) {
            "nsis".into()
        } else {
            "app".into()
        }
    };
    let identity = InstallationIdentity {
        product: crate::PRODUCT.into(),
        os: os.into(),
        arch: std::env::consts::ARCH.into(),
        channel,
    };
    identity.validate()?;
    Ok(identity)
}

#[tauri::command]
pub fn update_identity(app: tauri::AppHandle) -> Result<InstallationIdentity> {
    identity(&app)
}
