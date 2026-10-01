use crate::{AppError, MediaItem, Result, Space};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
/// The original Manager catalog includes every NFO, independent of card eligibility.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ManagerRow {
    pub item: MediaItem,
    pub series_title: String,
}

pub fn manager_catalog(mut items: Vec<MediaItem>) -> Vec<ManagerRow> {
    use std::{collections::HashMap, path::Path};
    items.sort_by(|a, b| a.path.cmp(&b.path));
    // Match the original cache lookup for an ancestor's tvshow.nfo. An unrelated
    // root, IMDb match, or similarly named file cannot supply a series title.
    let shows: HashMap<_, _> = items
        .iter()
        .filter(|item| item.kind == "Series")
        .map(|item| {
            (
                (item.root_id.clone(), item.path.clone()),
                item.title.trim().to_owned(),
            )
        })
        .collect();
    items
        .into_iter()
        .map(|item| {
            let series_title = if item.kind == "Series" {
                item.title.trim().to_owned()
            } else if !item.show_title.trim().is_empty() {
                item.show_title.trim().to_owned()
            } else {
                Path::new(&item.path)
                    .parent()
                    .into_iter()
                    .flat_map(Path::ancestors)
                    .find_map(|directory| {
                        shows.get(&(
                            item.root_id.clone(),
                            directory.join("tvshow.nfo").to_string_lossy().into_owned(),
                        ))
                    })
                    .cloned()
                    .unwrap_or_default()
            };
            ManagerRow { item, series_title }
        })
        .collect()
}
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec_filter: Option<SpecFilter>,
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
#[serde(rename_all = "kebab-case")]
pub enum SpecFilter {
    All,
    Ready,
    Missing,
    Error,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
pub struct CatalogSummary {
    pub total: u32,
    pub movie: u32,
    pub tv: u32,
    pub errors: u32,
    pub displayable: u32,
    pub web_eligible: u32,
    pub episodes_excluded: u32,
    pub generated_at: Option<String>,
    pub roots_configured: bool,
    // Matches the v4.1.0 status path: an unreadable completed summary is
    // visible without hiding a separately readable catalog snapshot.
    pub index_error: Option<AppError>,
}
pub fn summary(items: &[MediaItem]) -> CatalogSummary {
    let count = |value: usize| u32::try_from(value).unwrap_or(u32::MAX);
    let valid_id = regex::Regex::new(r"^tt\d{5,12}$").expect("IMDb pattern");
    let eligible = |item: &&MediaItem| {
        item.error.is_none() && !item.specs.is_empty() && valid_id.is_match(item.imdb.trim())
    };
    CatalogSummary {
        total: count(items.len()),
        movie: count(
            items
                .iter()
                .filter(|item| item.space == Space::Movie)
                .count(),
        ),
        tv: count(items.iter().filter(|item| item.space == Space::Tv).count()),
        errors: count(items.iter().filter(|item| item.error.is_some()).count()),
        displayable: count(crate::emby::public_index(items, String::new()).items.len()),
        web_eligible: count(
            items
                .iter()
                .filter(eligible)
                .filter(|item| matches!(item.kind.as_str(), "Movie" | "Series"))
                .count(),
        ),
        episodes_excluded: count(
            items
                .iter()
                .filter(eligible)
                .filter(|item| matches!(item.kind.as_str(), "Season" | "Episode"))
                .count(),
        ),
        generated_at: None,
        roots_configured: false,
        index_error: None,
    }
}
/// Product-window preferences. An absent value preserves the serialized request
/// identity of the earlier rewrite UI and does not invalidate operation replays.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
pub struct PresentationState {
    pub split_basis_points: u16,
    pub task_height: u16,
    pub task_open: bool,
    pub task_history: bool,
    pub inspector_tab: InspectorTab,
    pub current_movie: Option<String>,
    pub current_tv: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS, Default)]
#[serde(rename_all = "kebab-case")]
pub enum InspectorTab {
    #[default]
    Overview,
    Specs,
    Tags,
}
impl Default for PresentationState {
    fn default() -> Self {
        Self {
            split_basis_points: 4200,
            task_height: 240,
            task_open: false,
            task_history: false,
            inspector_tab: InspectorTab::Overview,
            current_movie: None,
            current_tv: None,
        }
    }
}
impl PresentationState {
    fn validate(&self) -> Result<()> {
        if !(100..=9900).contains(&self.split_basis_points)
            || !(190..=4000).contains(&self.task_height)
        {
            return Err(AppError::new(
                "view-layout-bounds",
                "Invalid window layout preference",
            ));
        }
        for id in [&self.current_movie, &self.current_tv]
            .into_iter()
            .flatten()
        {
            if id.is_empty() || id.len() > 256 || id.chars().any(char::is_control) {
                return Err(AppError::new(
                    "view-state-id",
                    "Invalid current item identifier",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
pub struct UiState {
    pub revision: u32,
    pub active_space: Space,
    pub movie: LibraryView,
    pub tv: LibraryView,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<PresentationState>,
}
impl Default for UiState {
    fn default() -> Self {
        Self {
            revision: 0,
            active_space: Space::Movie,
            movie: LibraryView::default(),
            tv: LibraryView::default(),
            presentation: None,
        }
    }
}
impl UiState {
    pub fn validate(&self) -> Result<()> {
        if let Some(presentation) = &self.presentation {
            presentation.validate()?;
        }
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
        && (view.roots.is_empty() || view.roots.contains(&item.root_id))
        && (!view.errors || item.error.is_some())
        && match view.spec_filter {
            None | Some(SpecFilter::All) => true,
            Some(SpecFilter::Ready) => item.error.is_none() && !item.specs.is_empty(),
            Some(SpecFilter::Missing) => item.error.is_none() && item.specs.is_empty(),
            Some(SpecFilter::Error) => item.error.is_some(),
        }
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
