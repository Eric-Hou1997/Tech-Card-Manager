//! Read-only Emby installation discovery. Missing paths remain explicit findings.
use crate::{paths, AppError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Environment {
    pub web: String,
    pub data: Option<String>,
    pub endpoint: String,
    pub version: Option<String>,
    pub issues: Vec<String>,
}
pub fn inspect(path: &Path) -> Result<Environment> {
    let selected = paths::checked(path)?;
    let candidates = [
        selected.clone(),
        selected.join("system/dashboard-ui"),
        selected.join("dashboard-ui"),
        selected.join("Contents/Resources/dashboard-ui"),
        selected.join("Contents/MacOS/system/dashboard-ui"),
    ];
    let mut found = Vec::new();
    for candidate in candidates {
        if candidate.join("index.html").is_file() {
            let web = paths::checked(&candidate)?;
            let file = paths::within(&web, &web.join("index.html"))?;
            let meta = std::fs::metadata(&file).map_err(|e| AppError::new("emby-discovery", e))?;
            if meta.len() > 4 * 1024 * 1024 {
                return Err(
                    AppError::new("emby-discovery", "Emby index is unexpectedly large")
                        .at(file.display()),
                );
            }
            let data = std::fs::read(&file).map_err(|e| AppError::new("emby-discovery", e))?;
            crate::emby::clean_index(&data)?;
            if !found.contains(&web) {
                found.push(web);
            }
        }
    }
    if found.len() != 1 {
        return Err(AppError::new(
            "emby-discovery",
            "Select one Emby installation or its web directory",
        )
        .at(selected.display()));
    }
    let web = found.remove(0);
    let data = web
        .ancestors()
        .take(4)
        .map(|p| p.join("programdata"))
        .find(|p| p.join("config").is_dir())
        .map(|p| paths::checked(&p))
        .transpose()?;
    Ok(Environment {
        web: web.to_string_lossy().into(),
        data: data.map(|p| p.to_string_lossy().into()),
        endpoint: "http://127.0.0.1:8096".into(),
        version: None,
        issues: vec!["server-version-unverified".into()],
    })
}
pub fn discover(
    os: &str,
    home: &Path,
    appdata: Option<&Path>,
    local: Option<&Path>,
) -> Vec<Environment> {
    let mut paths: Vec<PathBuf> = match os {
        "windows" => [appdata, local]
            .into_iter()
            .flatten()
            .map(|p| p.join("Emby-Server"))
            .collect(),
        "macos" => vec![
            PathBuf::from("/Applications/EmbyServer.app"),
            home.join("Applications/EmbyServer.app"),
        ],
        "linux" => vec![
            PathBuf::from("/opt/emby-server"),
            PathBuf::from("/usr/lib/emby-server"),
        ],
        _ => vec![],
    };
    paths.sort();
    paths.dedup();
    let mut found = Vec::new();
    for path in paths.into_iter().filter(|p| p.exists()) {
        match inspect(&path) {
            Ok(mut item) => {
                if item.data.is_none() && os == "linux" && Path::new("/var/lib/emby").is_dir() {
                    item.data = Some("/var/lib/emby".into());
                }
                if !found.iter().any(|old: &Environment| old.web == item.web) {
                    found.push(item);
                }
            }
            Err(e) => found.push(Environment {
                web: path.to_string_lossy().into(),
                issues: vec![e.to_string()],
                ..Default::default()
            }),
        }
    }
    found
}
pub fn endpoint(value: &str) -> Result<url::Url> {
    let mut url = url::Url::parse(value).map_err(|e| AppError::new("emby-url", e))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AppError::new(
            "emby-url",
            "Use an HTTP(S) server address without credentials, query or fragment",
        ));
    }
    let path = url.path().trim_end_matches('/').to_string();
    url.set_path(&(path + "/emby/System/Info/Public"));
    Ok(url)
}
pub fn server_version(environment: &mut Environment, value: &serde_json::Value) -> Result<()> {
    let version = value
        .get("Version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::new("emby-server-response", "Missing server version"))?;
    if version.split('.').count() != 4
        || !version.split('.').all(|part| part.parse::<u32>().is_ok())
    {
        return Err(AppError::new(
            "emby-server-response",
            "Invalid Emby version",
        ));
    }
    environment.version = Some(version.into());
    environment
        .issues
        .retain(|i| i != "server-version-unverified" && i != "server-version-not-accepted");
    if version != "4.9.5.0" {
        environment
            .issues
            .push("server-version-not-accepted".into());
    }
    Ok(())
}
