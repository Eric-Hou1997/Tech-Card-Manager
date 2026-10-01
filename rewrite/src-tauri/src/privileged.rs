//! Manager owns authorization, pipes, publication worker and child lifetime.
use product_core::{
    maintenance::{Command, Outcome},
    AppError, Result,
};
#[cfg(any(target_os = "linux", all(test, unix)))]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;
#[cfg(any(target_os = "linux", target_os = "macos", windows, all(test, unix)))]
#[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
mod worker;
#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub use worker::Connection;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub struct Connection;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
impl Connection {
    pub fn request(&self, _command: Command) -> Result<Outcome> {
        Err(unavailable())
    }
    pub fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn unavailable() -> AppError {
    AppError::new(
        "maintenance-platform-pending",
        "The signed native permission helper for this platform is not yet available",
    )
}
pub fn launch(
    web: &std::path::Path,
    store: std::sync::Arc<product_core::store::Store>,
) -> Result<Connection> {
    #[cfg(target_os = "linux")]
    {
        linux::launch(web, store)
    }
    #[cfg(target_os = "macos")]
    {
        macos::launch(web, store)
    }
    #[cfg(windows)]
    {
        windows::launch(web, store)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (web, store);
        Err(unavailable())
    }
}
pub fn available() -> bool {
    cfg!(any(target_os = "linux", target_os = "macos", windows))
}
