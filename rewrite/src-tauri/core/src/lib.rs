pub mod card_service;
pub mod contracts;
pub mod emby;
pub mod install;
pub mod library;
pub mod migration;
pub mod paths;
pub mod services;
pub mod store;
pub mod update;
pub use contracts::*;
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

pub mod tv;

pub mod lifecycle;

pub mod startup_file;

pub mod emby_libraries;

pub mod maintenance;

#[cfg(unix)]
pub mod maintenance_linux;
#[cfg(unix)]
pub mod maintenance_pipe;
