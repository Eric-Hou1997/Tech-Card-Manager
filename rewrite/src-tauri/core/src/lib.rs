pub mod card_service;
pub mod contracts;
pub mod emby;
pub mod install;
mod job_text;
pub mod legacy_agent;
pub mod legacy_components;
pub mod legacy_migration;
pub mod legacy_process;
pub mod legacy_startup;
pub mod legacy_task;
pub mod library;
pub mod manual_update;
pub mod migration;
pub mod paths;
pub mod services;
pub mod store;
pub mod update;
pub use contracts::*;
/// Selected only by the reviewed release recipe; development stays isolated.
pub const OFFICIAL_RELEASE: bool = match option_env!("TCM_RELEASE_BUILD") {
    Some(value) => matches!(value.as_bytes(), [b'1']),
    None => false,
};
pub const MANAGER_BINARY_NAME: &str = if OFFICIAL_RELEASE {
    "Tech-Card-Manager"
} else {
    "tcm-validation"
};
use sha2::{Digest, Sha256};
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn ownership_key(value: &str) -> String {
    value.to_lowercase()
}

pub(crate) fn collect_specs(out: &mut Specs, key: &str, values: Vec<String>) {
    out.insert(key.into(), values);
}

pub mod emby_environment;

pub mod ui;

pub mod lifecycle;

pub mod startup_file;

pub mod emby_libraries;

pub mod maintenance;

#[cfg(unix)]
pub mod maintenance_linux;
#[cfg(target_os = "macos")]
pub mod maintenance_macos;
#[cfg(unix)]
pub mod maintenance_pipe;

#[cfg(windows)]
pub mod maintenance_windows;
pub mod windows_arguments;
#[cfg(windows)]
pub mod windows_authorization;
#[cfg(windows)]
pub mod windows_security;
#[cfg(windows)]
pub mod windows_startup;
#[cfg(windows)]
pub mod windows_transport;

pub mod diagnostics;

pub mod incremental;

pub mod history;

pub mod folders;
pub mod languages;
