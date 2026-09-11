//! TV grouping uses physical path ancestry within a configured root, never IMDb ID.
use crate::{hash, ui::LibraryView, MediaItem, Space};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};
use ts_rs::TS;
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct TvRow {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub depth: u32,
    pub expandable: bool,
    pub member_count: u32,
    pub selected_count: u32,
    pub item: Option<MediaItem>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct TvPage {
    pub total: u32,
    pub rows: Vec<TvRow>,
}
struct Node {
    id: String,
    name: String,
    kind: String,
    item: Option<MediaItem>,
    children: Vec<Node>,
}
impl Node {
    fn ids(&self, out: &mut Vec<String>) {
        if let Some(item) = &self.item {
            if item.kind != "Season" {
                out.push(item.id.clone());
            }
        }
        for child in &self.children {
            child.ids(out);
        }
    }
    fn matching(&self, view: &LibraryView) -> bool {
        self.item
            .as_ref()
            .is_some_and(|item| crate::ui::matches(item, &Space::Tv, view))
            || self.children.iter().any(|n| n.matching(view))
    }
    fn find(&self, id: &str) -> Option<&Node> {
        if self.id == id {
            Some(self)
        } else {
            self.children.iter().find_map(|c| c.find(id))
        }
    }
}
fn forest(items: &[MediaItem], view: &LibraryView) -> Vec<Node> {
    let items: Vec<_> = items.iter().filter(|i| i.space == Space::Tv).collect();
    let series: Vec<_> = items
        .iter()
        .copied()
        .filter(|i| i.kind == "Series")
        .collect();
    let mut by_directory: BTreeMap<(String, std::path::PathBuf), Vec<&MediaItem>> = BTreeMap::new();
    for item in &series {
        if let Some(parent) = Path::new(&item.path).parent() {
            by_directory
                .entry((item.root_id.clone(), parent.to_path_buf()))
                .or_default()
                .push(item);
        }
    }
    let mut trees: BTreeMap<String, Node> = BTreeMap::new();
    for item in &series {
        trees.insert(
            item.id.clone(),
            Node {
                id: item.id.clone(),
                name: item.title.clone(),
                kind: "series".into(),
                item: Some((*item).clone()),
                children: vec![],
            },
        );
    }
    for item in items.iter().copied().filter(|i| i.kind != "Series") {
        let parent = Path::new(&item.path)
            .parent()
            .into_iter()
            .flat_map(Path::ancestors)
            .find_map(|directory| {
                by_directory.get(&(item.root_id.clone(), directory.to_path_buf()))
            })
            .filter(|candidates| candidates.len() == 1)
            .map(|candidates| candidates[0]);
        let root_id = parent
            .map(|p| p.id.clone())
            .unwrap_or_else(|| format!("orphan-{}", item.root_id));
        let root = trees.entry(root_id.clone()).or_insert_with(|| Node {
            id: root_id.clone(),
            name: String::new(),
            kind: "orphan".into(),
            item: None,
            children: vec![],
        });
        let season = if item.season.is_empty() {
            "unknown"
        } else {
            &item.season
        };
        let season_id = format!("season-{}", hash(format!("{root_id}:{season}").as_bytes()));
        let index = match root.children.iter().position(|n| n.id == season_id) {
            Some(index) => index,
            None => {
                root.children.push(Node {
                    id: season_id.clone(),
                    name: season.into(),
                    kind: "season".into(),
                    item: None,
                    children: vec![],
                });
                root.children.len() - 1
            }
        };
        let node = &mut root.children[index];
        if item.kind == "Season" && node.item.is_none() {
            node.item = Some(item.clone());
        } else {
            node.children.push(Node {
                id: item.id.clone(),
                name: item.title.clone(),
                kind: "episode".into(),
                item: Some(item.clone()),
                children: vec![],
            });
        }
    }
    let mut roots: Vec<_> = trees.into_values().collect();
    roots.sort_by(|a, b| {
        let av = a.item.as_ref();
        let bv = b.item.as_ref();
        let field = |item: Option<&MediaItem>, name: &str| match view.sort {
            crate::ui::Sort::Title => name.to_lowercase(),
            crate::ui::Sort::Year => item.map(|i| i.year.clone()).unwrap_or_default(),
            crate::ui::Sort::Path => item.map(|i| i.path.clone()).unwrap_or_default(),
            crate::ui::Sort::Status => item.is_some_and(|i| i.error.is_some()).to_string(),
        };
        let order = field(av, &a.name)
            .cmp(&field(bv, &b.name))
            .then_with(|| a.id.cmp(&b.id));
        if view.descending {
            order.reverse()
        } else {
            order
        }
    });
    for root in &mut roots {
        root.children
            .sort_by_key(|n| (n.name.parse::<u32>().unwrap_or(u32::MAX), n.name.clone()));
        for season in &mut root.children {
            season.children.sort_by_key(|n| {
                n.item
                    .as_ref()
                    .map(|i| (i.episode.parse::<u32>().unwrap_or(u32::MAX), i.path.clone()))
            });
        }
    }
    roots
}
pub fn page(items: &[MediaItem], view: &LibraryView) -> TvPage {
    let roots = forest(items, view);
    let selected: std::collections::HashSet<_> = view.selected.iter().collect();
    let expanded: std::collections::HashSet<_> = view.expanded.iter().collect();
    let mut rows = Vec::new();
    fn visit(
        node: &Node,
        depth: u32,
        view: &LibraryView,
        selected: &std::collections::HashSet<&String>,
        expanded: &std::collections::HashSet<&String>,
        rows: &mut Vec<TvRow>,
    ) {
        if !node.matching(view) {
            return;
        }
        let mut members = Vec::new();
        node.ids(&mut members);
        rows.push(TvRow {
            id: node.id.clone(),
            name: node.name.clone(),
            kind: node.kind.clone(),
            depth,
            expandable: !node.children.is_empty(),
            member_count: members.len().try_into().unwrap_or(u32::MAX),
            selected_count: members
                .iter()
                .filter(|id| selected.contains(id))
                .count()
                .try_into()
                .unwrap_or(u32::MAX),
            item: node.item.clone(),
        });
        if expanded.contains(&node.id) {
            for child in &node.children {
                visit(child, depth + 1, view, selected, expanded, rows);
            }
        }
    }
    for root in &roots {
        visit(root, 0, view, &selected, &expanded, &mut rows);
    }
    TvPage {
        total: rows.len().try_into().unwrap_or(u32::MAX),
        rows: rows
            .into_iter()
            .skip(view.offset as usize)
            .take(100)
            .collect(),
    }
}
pub fn members(items: &[MediaItem], id: &str) -> crate::Result<Vec<String>> {
    for root in forest(items, &LibraryView::default()) {
        if let Some(node) = root.find(id) {
            let mut members = Vec::new();
            node.ids(&mut members);
            members.sort();
            members.dedup();
            return Ok(members);
        }
    }
    Err(crate::AppError::new(
        "tv-node-missing",
        "TV group changed; refresh its view before selecting",
    ))
}
