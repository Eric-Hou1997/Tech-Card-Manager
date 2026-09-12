//! Manager owns authorization, pipes, publication worker and child lifetime.
use product_core::{
    maintenance::{Command, Outcome},
    AppError, Result,
};
#[cfg(any(target_os = "linux", all(test, unix)))]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::Connection;
#[cfg(not(target_os = "linux"))]
pub struct Connection;
#[cfg(not(target_os = "linux"))]
impl Connection {
    pub fn request(&self, _command: Command) -> Result<Outcome> {
        Err(unavailable())
    }
    pub fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}
#[cfg(not(target_os = "linux"))]
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
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (web, store);
        Err(unavailable())
    }
}
pub fn available() -> bool {
    cfg!(target_os = "linux")
}
