use crate::desktop::Desktop;
use product_core::{
    update::{InstallationIdentity, UpdateArtifact, UpdateCatalog, UpdateProgress},
    *,
};
#[cfg(not(windows))]
use std::path::PathBuf;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::Duration,
};
use tauri::{Emitter, Manager, State};
#[cfg(windows)]
use tauri_plugin_updater::UpdaterExt;
#[derive(Clone)]
struct Pending {
    id: String,
    identity: InstallationIdentity,
    artifact: UpdateArtifact,
}
pub struct Updates {
    pending: Mutex<Option<Pending>>,
    busy: AtomicBool,
    cancel: AtomicBool,
}
impl Default for Updates {
    fn default() -> Self {
        Self {
            pending: Mutex::new(None),
            busy: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
        }
    }
}
struct Busy<'a>(&'a AtomicBool);
impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}
fn error(code: &str, e: impl ToString) -> AppError {
    AppError::new(code, e)
}
fn public_key() -> Result<&'static str> {
    option_env!("TECH_CARD_UPDATE_PUBLIC_KEY")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            error(
                "update-trust-not-configured",
                "TCM trusted update key has not been supplied",
            )
        })
}
fn endpoint() -> Result<&'static str> {
    option_env!("REWRITE_UPDATE_FEED")
        .filter(|s| s.starts_with("https://"))
        .ok_or_else(|| {
            error(
                "update-channel-not-configured",
                "This build has no configured signed update channel",
            )
        })
}
fn network(e: reqwest::Error) -> AppError {
    if e.is_timeout() {
        return error("update-timeout", "Update request timed out");
    }
    let message = e.to_string();
    let lower = message.to_lowercase();
    error(
        if lower.contains("dns") {
            "update-dns"
        } else if lower.contains("proxy") {
            "update-proxy"
        } else if e.is_connect() {
            "update-offline"
        } else {
            "update-network"
        },
        message,
    )
}
fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .https_only(true)
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(network)
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
fn record(app: &tauri::AppHandle, state: &UpdateProgress) -> Result<()> {
    app.state::<Desktop>().store.update_progress(state)?;
    app.emit("update-progress", state)
        .map_err(|e| error("update-event", e))
}
#[tauri::command]
pub fn update_identity(app: tauri::AppHandle) -> Result<InstallationIdentity> {
    identity(&app)
}
#[tauri::command]
pub fn update_status(state: State<'_, Desktop>) -> Result<Option<UpdateProgress>> {
    let value = state.store.preferences("update-state")?;
    if value.get("operation_id").is_none() {
        return Ok(None);
    }
    serde_json::from_value(value).map(Some).map_err(Into::into)
}
#[tauri::command]
pub fn update_cancel(state: State<'_, Updates>) {
    state.cancel.store(true, Ordering::SeqCst);
}
#[tauri::command]
pub async fn update_check(id: String, app: tauri::AppHandle) -> Result<UpdateProgress> {
    let updates = app.state::<Updates>();
    if updates.busy.swap(true, Ordering::SeqCst) {
        return Err(error(
            "update-busy",
            "An update operation is already running",
        ));
    }
    let _busy = Busy(&updates.busy);
    let mut progress = UpdateProgress {
        operation_id: id.clone(),
        phase: "checking".into(),
        version: None,
        downloaded: "0".into(),
        total: None,
        error: None,
    };
    record(&app, &progress)?;
    let result = async {
        public_key()?;
        let identity = identity(&app)?;
        if identity.package_managed() {
            return Err(error(
                "update-package-manager",
                "Upgrade from the configured system software repository",
            ));
        }
        let response = client()?.get(endpoint()?).send().await.map_err(network)?;
        if !response.status().is_success() {
            return Err(product_core::update::http_failure(
                response.status().as_u16(),
                response
                    .headers()
                    .get("x-ratelimit-remaining")
                    .and_then(|v| v.to_str().ok()),
                response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok()),
            ));
        }
        if response
            .content_length()
            .is_some_and(|n| n > 2 * 1024 * 1024)
        {
            return Err(error(
                "update-catalog-size",
                "Update catalog exceeds size limit",
            ));
        }
        let data = response.bytes().await.map_err(network)?;
        if data.len() > 2 * 1024 * 1024 {
            return Err(error(
                "update-catalog-size",
                "Update catalog exceeds size limit",
            ));
        }
        let catalog: UpdateCatalog = serde_json::from_slice(&data)?;
        let selected = product_core::update::select(
            &catalog,
            &identity,
            &app.package_info().version.to_string(),
        )?
        .cloned();
        let mut pending = updates
            .pending
            .lock()
            .map_err(|e| error("update-state", e))?;
        if let Some(artifact) = selected {
            progress.phase = "available".into();
            progress.version = Some(artifact.version.clone());
            *pending = Some(Pending {
                id,
                identity,
                artifact,
            });
        } else {
            progress.phase = "up-to-date".into();
            *pending = None;
        }
        Ok::<(), AppError>(())
    }
    .await;
    if let Err(e) = result {
        progress.phase = "failed".into();
        progress.error = Some(e.clone());
        record(&app, &progress)?;
        return Err(e);
    }
    record(&app, &progress)?;
    Ok(progress)
}
#[cfg(not(windows))]
fn installation_path(app: &tauri::AppHandle, channel: &str) -> Result<PathBuf> {
    #[cfg(target_os = "linux")]
    if channel == "appimage" {
        return app
            .env()
            .appimage
            .clone()
            .map(PathBuf::from)
            .ok_or_else(|| error("update-installation-path", "Missing AppImage path"));
    }
    let _ = (app, channel);
    let exe = std::env::current_exe().map_err(|e| error("update-installation-path", e))?;
    if cfg!(target_os = "macos") {
        exe.ancestors()
            .find(|p| p.extension().is_some_and(|s| s == "app"))
            .map(|p| p.to_path_buf())
            .ok_or_else(|| {
                error(
                    "update-installation-path",
                    "Run the installed application bundle",
                )
            })
    } else {
        Ok(exe)
    }
}
#[tauri::command]
pub async fn update_install(id: String, app: tauri::AppHandle) -> Result<UpdateProgress> {
    let updates = app.state::<Updates>();
    if updates.busy.swap(true, Ordering::SeqCst) {
        return Err(error(
            "update-busy",
            "An update operation is already running",
        ));
    }
    let _busy = Busy(&updates.busy);
    updates.cancel.store(false, Ordering::SeqCst);
    let pending = updates
        .pending
        .lock()
        .map_err(|e| error("update-state", e))?
        .clone()
        .filter(|p| p.id == id)
        .ok_or_else(|| {
            error(
                "update-check-required",
                "Check for updates again before installation",
            )
        })?;
    let mut progress = UpdateProgress {
        operation_id: id.clone(),
        phase: "downloading".into(),
        version: Some(pending.artifact.version.clone()),
        downloaded: "0".into(),
        total: None,
        error: None,
    };
    record(&app, &progress)?;
    let result = async {
        let mut response = client()?
            .get(&pending.artifact.url)
            .send()
            .await
            .map_err(network)?;
        if !response.status().is_success() {
            return Err(product_core::update::http_failure(
                response.status().as_u16(),
                None,
                None,
            ));
        }
        let total = response.content_length();
        if total.is_some_and(|n| n > 2 * 1024 * 1024 * 1024) {
            return Err(error("update-download-size", "Update exceeds 2 GiB"));
        }
        progress.total = total.map(|n| n.to_string());
        let mut bytes = Vec::new();
        let mut last_report = 0usize;
        while let Some(chunk) = response.chunk().await.map_err(network)? {
            if updates.cancel.load(Ordering::SeqCst) {
                return Err(error(
                    "update-cancelled",
                    "Update download cancelled before installation",
                ));
            }
            if bytes.len() + chunk.len() > 2 * 1024 * 1024 * 1024 {
                return Err(error("update-download-size", "Update exceeds 2 GiB"));
            }
            bytes.extend_from_slice(&chunk);
            if bytes.len() - last_report >= 1024 * 1024 {
                progress.downloaded = bytes.len().to_string();
                record(&app, &progress)?;
                last_report = bytes.len();
            }
        }
        if updates.cancel.load(Ordering::SeqCst) {
            return Err(error(
                "update-cancelled",
                "Update cancelled before installation",
            ));
        }
        progress.downloaded = bytes.len().to_string();
        progress.phase = "verifying".into();
        record(&app, &progress)?;
        if product_core::hash(&bytes) != pending.artifact.sha256.to_ascii_lowercase() {
            return Err(error(
                "update-checksum",
                "Downloaded bytes do not match the reviewed artifact",
            ));
        }
        product_core::update::verify_signature(public_key()?, &pending.artifact.signature, &bytes)?;
        let desktop = app.state::<Desktop>();
        desktop.store.freeze_for_update()?;
        let directory = app
            .path()
            .app_data_dir()
            .map_err(|e| error("update-data-directory", e))?
            .join("updates");
        std::fs::create_dir_all(&directory).map_err(|e| error("update-data-directory", e))?;
        let backup = directory.join(format!("{id}-database.sqlite"));
        desktop.store.snapshot_database(&backup)?;
        progress.phase = "installing".into();
        record(&app, &progress)?;
        #[cfg(windows)]
        {
            let native = app
                .updater_builder()
                .pubkey(public_key()?)
                .target(pending.identity.target())
                .endpoints(vec![endpoint()?
                    .parse()
                    .map_err(|e| error("update-url", e))?])
                .map_err(|e| error("update-installer", e))?
                .configure_client(|client| client.https_only(true))
                .build()
                .map_err(|e| error("update-installer", e))?
                .check()
                .await
                .map_err(|e| error("update-installer", e))?
                .ok_or_else(|| {
                    error(
                        "update-catalog-changed",
                        "Update catalog changed; check again",
                    )
                })?;
            if native.download_url.as_str() != pending.artifact.url
                || native.signature != pending.artifact.signature
                || native.version.trim_start_matches('v')
                    != pending.artifact.version.trim_start_matches('v')
            {
                return Err(error(
                    "update-catalog-changed",
                    "Installer catalog differs from reviewed artifact",
                ));
            }
            crate::prepare_update_exit(&app)?;
            native
                .install(&bytes)
                .map_err(|e| error("update-install-failed", e))?;
        }
        #[cfg(not(windows))]
        {
            let journal = directory.join(format!("{id}-application.json"));
            let target = installation_path(&app, &pending.identity.channel)?;
            product_core::install::stage(
                &journal,
                &id,
                &target,
                &bytes,
                &pending.identity.channel,
            )?;
            product_core::install::commit(&journal)?;
        }
        progress.phase = "installed-awaiting-health".into();
        record(&app, &progress)?;
        crate::prepare_update_exit(&app)?;
        app.restart();
        #[allow(unreachable_code)]
        Ok::<(), AppError>(())
    }
    .await;
    if let Err(e) = result {
        app.state::<crate::lifecycle::Lifecycle>()
            .allow_exit
            .store(false, Ordering::SeqCst);
        app.state::<Desktop>().store.unfreeze_after_update();
        progress.phase = if e.code == "update-cancelled" {
            "cancelled"
        } else {
            "failed"
        }
        .into();
        progress.error = Some(e.clone());
        record(&app, &progress)?;
        return Err(e);
    }
    Ok(progress)
}
