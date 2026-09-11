use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "kebab-case")]
pub enum Space {
    Movie,
    Tv,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
pub enum Locale {
    #[serde(rename = "zh-CN")]
    Simplified,
    #[serde(rename = "zh-Hant")]
    Traditional,
    #[serde(rename = "en-US")]
    English,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
pub struct LibraryRoot {
    pub id: String,
    pub space: Space,
    pub path: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
pub struct Configuration {
    pub revision: u32,
    pub locale: Locale,
    pub roots: Vec<LibraryRoot>,
}
impl Default for Configuration {
    fn default() -> Self {
        Self {
            revision: 0,
            locale: Locale::Simplified,
            roots: vec![],
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "kebab-case")]
pub enum TaskState {
    Requested,
    Running,
    Paused,
    Cancelled,
    Interrupted,
    Failed,
    Completed,
}
impl TaskState {
    pub fn terminal(&self) -> bool {
        matches!(self, Self::Cancelled | Self::Failed | Self::Completed)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
pub struct AppError {
    pub code: String,
    pub message: String,
    pub path: Option<String>,
    pub operation_id: Option<String>,
    pub retryable: bool,
}
impl AppError {
    pub fn new(code: &str, message: impl ToString) -> Self {
        Self {
            code: code.into(),
            message: message.to_string(),
            path: None,
            operation_id: None,
            retryable: false,
        }
    }
    pub fn at(mut self, path: impl ToString) -> Self {
        self.path = Some(path.to_string());
        self
    }
}
impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for AppError {}
impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        Self::new("database", e)
    }
}
impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::new("invalid-data", e)
    }
}
pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
pub struct ScanRequest {
    pub operation_id: String,
    pub space: Space,
    pub root_ids: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
pub struct TaskControl {
    pub operation_id: String,
    pub task_id: String,
    pub state: TaskState,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
pub struct Task {
    pub id: String,
    pub state: TaskState,
    pub locale: Locale,
    pub space: Space,
    pub roots: Vec<LibraryRoot>,
    #[serde(default)]
    pub attempt: u32,
    pub processed: u32,
    pub errors: u32,
    pub current_path: Option<String>,
    pub failure: Option<AppError>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "kebab-case")]
pub enum Ownership {
    External,
    Generated,
    Manual,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
pub struct Tag {
    pub value: String,
    pub ownership: Ownership,
    pub engine: String,
}
pub type Specs = BTreeMap<String, Vec<String>>;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
pub struct MediaItem {
    #[serde(default)]
    pub parser_revision: u32,
    pub id: String,
    pub root_id: String,
    pub space: Space,
    pub path: String,
    pub source_hash: String,
    pub title: String,
    pub year: String,
    pub imdb: String,
    pub kind: String,
    pub season: String,
    pub episode: String,
    pub specs: Specs,
    pub tags: Vec<Tag>,
    pub error: Option<AppError>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
pub struct CatalogQuery {
    pub space: Space,
    pub search: String,
    pub only_errors: bool,
    pub offset: u32,
    pub limit: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
pub struct CatalogPage {
    pub total: u32,
    pub items: Vec<MediaItem>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", content = "result", rename_all = "kebab-case")]
pub enum OperationResult {
    PathMappings(crate::emby_libraries::MappingSettings),
    Lifecycle(crate::lifecycle::SettingsOperation),
    Ui(crate::ui::UiReceipt),
    Configuration(Configuration),
    Update(crate::update::UpdateProgress),
    MigrationPlan(crate::migration::MigrationPlan),
    Migration(crate::migration::MigrationReceipt),
    Task(Task),
}

pub fn typescript() -> String {
    let declarations = [
        Space::decl(),
        Locale::decl(),
        LibraryRoot::decl(),
        Configuration::decl(),
        TaskState::decl(),
        AppError::decl(),
        ScanRequest::decl(),
        TaskControl::decl(),
        Task::decl(),
        Ownership::decl(),
        Tag::decl(),
        MediaItem::decl(),
        CatalogQuery::decl(),
        CatalogPage::decl(),
        crate::migration::LegacyFile::decl(),
        crate::migration::LegacyRoot::decl(),
        crate::migration::MigrationPlan::decl(),
        crate::migration::MigrationReceipt::decl(),
        crate::update::InstallationIdentity::decl(),
        crate::update::UpdateArtifact::decl(),
        crate::update::UpdateCatalog::decl(),
        crate::update::UpdateProgress::decl(),
        crate::ui::Sort::decl(),
        crate::ui::LibraryView::decl(),
        crate::ui::UiState::decl(),
        crate::ui::UiReceipt::decl(),
        crate::tv::TvRow::decl(),
        crate::tv::TvPage::decl(),
        crate::lifecycle::CloseAction::decl(),
        crate::lifecycle::Settings::decl(),
        crate::lifecycle::SettingsOperation::decl(),
        crate::emby_libraries::PathMapping::decl(),
        crate::emby_libraries::MappingSettings::decl(),
        crate::emby_libraries::DiscoveredLibrary::decl(),
        OperationResult::decl(),
    ];
    format!(
        "// Generated from Rust contracts. Do not edit.\n{}\n",
        declarations
            .into_iter()
            .map(|s| format!("export {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}
