use crate::{AppError, MediaItem, Result, Space};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Sort {
    #[default]
    Title,
    Year,
    Path,
    Status,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, Default)]
pub struct LibraryView {
    pub search: String,
    pub errors: bool,
    pub roots: Vec<String>,
    pub selected: Vec<String>,
    pub expanded: Vec<String>,
    pub offset: u32,
    pub sort: Sort,
    pub descending: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
pub struct UiState {
    pub revision: u32,
    pub active_space: Space,
    pub movie: LibraryView,
    pub tv: LibraryView,
}
impl Default for UiState {
    fn default() -> Self {
        Self {
            revision: 0,
            active_space: Space::Movie,
            movie: LibraryView::default(),
            tv: LibraryView::default(),
        }
    }
}
impl UiState {
    pub fn validate(&self) -> Result<()> {
        for view in [&self.movie, &self.tv] {
            if view.search.len() > 4096
                || view.roots.len() > 1000
                || view.selected.len() > 100_000
                || view.expanded.len() > 100_000
                || view.offset > 10_000_000
            {
                return Err(AppError::new(
                    "view-state-limit",
                    "View state exceeds supported size",
                ));
            }
            for values in [&view.roots, &view.selected, &view.expanded] {
                let mut ids = std::collections::HashSet::new();
                for id in values {
                    if id.is_empty() || id.len() > 256 || !ids.insert(id) {
                        return Err(AppError::new(
                            "view-state-id",
                            "Invalid or repeated view identifier",
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}
pub fn matches(item: &MediaItem, space: &Space, view: &LibraryView) -> bool {
    item.space == *space
        && (!view.errors || item.error.is_some())
        && (view.search.is_empty()
            || format!("{} {} {} {}", item.title, item.year, item.imdb, item.path)
                .to_lowercase()
                .contains(&view.search.to_lowercase()))
}
pub fn sort(items: &mut [MediaItem], view: &LibraryView) {
    items.sort_by(|a, b| {
        let order = match view.sort {
            Sort::Title => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
            Sort::Year => a.year.cmp(&b.year),
            Sort::Path => a.path.cmp(&b.path),
            Sort::Status => a.error.is_some().cmp(&b.error.is_some()),
        };
        let order = order
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.id.cmp(&b.id));
        if view.descending {
            order.reverse()
        } else {
            order
        }
    });
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
pub struct UiReceipt {
    pub revision: u32,
}
