use tcm_core::{store::Store, tv, ui::*, *};
#[test]
fn view_state_restarts_replays_and_rejects_stale_writes_without_cross_space_loss() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.sqlite");
    let store = Store::open(&path).unwrap();
    let mut state = store.ui_state().unwrap();
    state.movie.search = "电影".into();
    state.movie.sort = Sort::Year;
    state.tv.search = "剧集".into();
    state.tv.selected = vec!["episode-one".into()];
    state.active_space = Space::Tv;
    let receipt = store.save_ui_state("view-one", state.clone()).unwrap();
    assert_eq!(receipt.revision, 1);
    assert_eq!(
        store.save_ui_state("view-one", state.clone()).unwrap(),
        receipt
    );
    assert_eq!(
        store
            .save_ui_state("stale", state.clone())
            .unwrap_err()
            .code,
        "view-state-conflict"
    );
    state.movie.search = "changed".into();
    assert_eq!(
        store.save_ui_state("view-one", state).unwrap_err().code,
        "operation-conflict"
    );
    drop(store);
    let saved = Store::open(&path).unwrap().ui_state().unwrap();
    assert_eq!(saved.movie.search, "电影");
    assert_eq!(saved.movie.sort, Sort::Year);
    assert_eq!(saved.tv.search, "剧集");
    assert_eq!(saved.tv.selected, vec!["episode-one"]);
    assert_eq!(saved.active_space, Space::Tv);
}
fn item(id: &str, path: &str, kind: &str, season: &str, episode: &str) -> MediaItem {
    MediaItem {
        parser_revision: 1,
        id: id.into(),
        root_id: "tv-root".into(),
        space: Space::Tv,
        path: path.into(),
        source_hash: String::new(),
        title: id.into(),
        year: "2020".into(),
        imdb: "tt0061452".into(),
        kind: kind.into(),
        season: season.into(),
        episode: episode.into(),
        specs: Default::default(),
        tags: vec![],
        error: None,
    }
}
#[test]
fn same_imdb_folders_stay_separate_and_season_selection_includes_hidden_episodes_only() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path();
    let a = base.join("中文 A");
    let b = base.join("中文 B");
    let items = vec![
        item(
            "show-a",
            a.join("tvshow.nfo").to_str().unwrap(),
            "Series",
            "",
            "",
        ),
        item(
            "show-b",
            b.join("tvshow.nfo").to_str().unwrap(),
            "Series",
            "",
            "",
        ),
        item(
            "a10",
            a.join("Season 1/e10.nfo").to_str().unwrap(),
            "Episode",
            "1",
            "10",
        ),
        item(
            "a2",
            a.join("Season 1/e2.nfo").to_str().unwrap(),
            "Episode",
            "1",
            "2",
        ),
        item(
            "b1",
            b.join("Season 1/e1.nfo").to_str().unwrap(),
            "Episode",
            "1",
            "1",
        ),
    ];
    let mut view = LibraryView::default();
    assert_eq!(tv::page(&items, &view).total, 2);
    view.expanded = vec!["show-a".into()];
    let page = tv::page(&items, &view);
    let season = page
        .rows
        .iter()
        .find(|r| r.kind == "season")
        .unwrap()
        .id
        .clone();
    assert_eq!(tv::members(&items, &season).unwrap(), vec!["a10", "a2"]);
    view.expanded.push(season.clone());
    let page = tv::page(&items, &view);
    let episodes: Vec<_> = page
        .rows
        .iter()
        .filter(|r| r.kind == "episode")
        .map(|r| r.id.as_str())
        .collect();
    assert_eq!(episodes, vec!["a2", "a10"]);
    view.search = "a2".into();
    view.selected = tv::members(&items, &season).unwrap();
    let page = tv::page(&items, &view);
    assert!(page.rows.iter().any(|r| r.id == "show-a"));
    assert!(!page.rows.iter().any(|r| r.id == "show-b"));
    let group = page.rows.iter().find(|r| r.id == season).unwrap();
    assert_eq!((group.selected_count, group.member_count), (2, 2));
    assert!(!page.rows.iter().any(|r| r.id == "a10"));
}
#[test]
fn missing_series_stays_inspectable_under_an_orphan_group() {
    let items = vec![item(
        "orphan",
        "/library/Unknown/e1.nfo",
        "Episode",
        "1",
        "1",
    )];
    let mut view = LibraryView::default();
    let root = tv::page(&items, &view).rows.remove(0);
    assert_eq!(root.kind, "orphan");
    assert_eq!(tv::members(&items, &root.id).unwrap(), vec!["orphan"]);
    view.expanded.push(root.id);
    assert_eq!(tv::page(&items, &view).rows.len(), 2);
}

#[test]
fn ambiguous_nearest_series_is_not_assigned_to_an_unrelated_show() {
    let items = vec![
        item("outer", "/library/tvshow.nfo", "Series", "", ""),
        item("one", "/library/duplicate/one.nfo", "Series", "", ""),
        item("two", "/library/duplicate/two.nfo", "Series", "", ""),
        item(
            "episode",
            "/library/duplicate/Season 1/e1.nfo",
            "Episode",
            "1",
            "1",
        ),
    ];
    let page = tv::page(&items, &LibraryView::default());
    let orphan = page.rows.iter().find(|r| r.kind == "orphan").unwrap();
    assert_eq!(tv::members(&items, &orphan.id).unwrap(), vec!["episode"]);
    assert_eq!(tv::members(&items, "outer").unwrap(), vec!["outer"]);
    assert_eq!(tv::members(&items, "one").unwrap(), vec!["one"]);
}
