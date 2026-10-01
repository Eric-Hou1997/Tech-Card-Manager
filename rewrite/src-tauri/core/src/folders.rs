//! Original Manager directory rows, separate from per-space scanner roots.
use crate::*;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "kebab-case")]
pub enum FolderKind {
    Auto,
    Movies,
    Tv,
    Mixed,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "kebab-case")]
pub enum FolderSource {
    Auto,
    Manual,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(deny_unknown_fields)]
pub struct MediaFolder {
    pub id: String,
    pub path: String,
    pub name: String,
    pub kind: FolderKind,
    pub source: FolderSource,
    pub enabled: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(deny_unknown_fields)]
pub struct FolderSettings {
    pub revision: u32,
    pub folders: Vec<MediaFolder>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
pub struct FolderReceipt {
    pub settings: FolderSettings,
    pub configuration: Configuration,
}
#[derive(Debug, Clone, Serialize)]
pub enum ManagerScanScope {
    Space(Space),
    Folder(String),
}
impl FolderKind {
    pub fn spaces(&self) -> Vec<Space> {
        match self {
            Self::Movies => vec![Space::Movie],
            Self::Tv => vec![Space::Tv],
            _ => vec![Space::Movie, Space::Tv],
        }
    }
}
pub(crate) fn from_configuration(
    current: &Configuration,
    previous: Option<FolderSettings>,
) -> FolderSettings {
    let mut folders = previous.map(|v| v.folders).unwrap_or_default();
    folders.retain(|folder| {
        !folder.enabled || current.roots.iter().any(|root| root.path == folder.path)
    });
    for root in &current.roots {
        if let Some(folder) = folders.iter_mut().find(|folder| folder.path == root.path) {
            folder.enabled = true;
            continue;
        }
        folders.push(MediaFolder {
            id: hash(root.path.as_bytes()),
            path: root.path.clone(),
            name: std::path::Path::new(&root.path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            kind: FolderKind::Auto,
            source: FolderSource::Manual,
            enabled: true,
        });
    }
    for folder in &mut folders {
        if !folder.enabled {
            continue;
        }
        let movie = current
            .roots
            .iter()
            .any(|r| r.path == folder.path && r.space == Space::Movie);
        let tv = current
            .roots
            .iter()
            .any(|r| r.path == folder.path && r.space == Space::Tv);
        folder.kind = match (movie, tv) {
            (true, false) => FolderKind::Movies,
            (false, true) => FolderKind::Tv,
            _ => match folder.kind {
                FolderKind::Auto => FolderKind::Auto,
                _ => FolderKind::Mixed,
            },
        };
    }
    FolderSettings {
        revision: current.revision,
        folders,
    }
}
