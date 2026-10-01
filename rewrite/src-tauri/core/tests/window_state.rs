use tcm_core::{store::Store, ui::*, Space};

#[test]
fn window_preferences_restore_atomically_with_independent_media_state() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("window.sqlite");
    let store = Store::open(&path).unwrap();
    let mut state = store.ui_state().unwrap();
    state.movie.search = "电影".into();
    state.tv.selected = vec!["episode-one".into()];
    state.active_space = Space::Tv;
    state.presentation = Some(PresentationState {
        split_basis_points: 4425,
        task_height: 315,
        task_open: true,
        task_history: true,
        inspector_tab: InspectorTab::Tags,
        current_movie: Some("movie-one".into()),
        current_tv: Some("episode-one".into()),
    });
    let receipt = store.save_ui_state("layout-one", state.clone()).unwrap();
    assert_eq!(
        store.save_ui_state("layout-one", state.clone()).unwrap(),
        receipt
    );
    let mut invalid = store.ui_state().unwrap();
    invalid.presentation.as_mut().unwrap().task_height = 0;
    assert_eq!(
        store.save_ui_state("invalid", invalid).unwrap_err().code,
        "view-layout-bounds"
    );
    let mut changed = state.clone();
    changed.presentation.as_mut().unwrap().task_open = false;
    assert_eq!(
        store
            .save_ui_state("stale", changed.clone())
            .unwrap_err()
            .code,
        "view-state-conflict"
    );
    assert_eq!(
        store.save_ui_state("layout-one", changed).unwrap_err().code,
        "operation-conflict"
    );
    drop(store);
    state.revision = receipt.revision;
    assert_eq!(Store::open(&path).unwrap().ui_state().unwrap(), state);
}

#[test]
fn earlier_rewrite_state_keeps_exact_request_fingerprint_and_defaults() {
    #[derive(serde::Serialize)]
    struct PreviousState {
        revision: u32,
        active_space: Space,
        movie: LibraryView,
        tv: LibraryView,
    }
    let previous = serde_json::to_vec(&PreviousState {
        revision: 17,
        active_space: Space::Tv,
        movie: LibraryView::default(),
        tv: LibraryView::default(),
    })
    .unwrap();
    let current: UiState = serde_json::from_slice(&previous).unwrap();
    assert_eq!(current.presentation, None);
    assert_eq!(serde_json::to_vec(&current).unwrap(), previous);
    assert_eq!(PresentationState::default().split_basis_points, 4200);
    assert_eq!(PresentationState::default().task_height, 240);
}

#[test]
fn malformed_layout_and_current_item_never_replace_a_valid_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(&temp.path().join("window.sqlite")).unwrap();
    let original = store.ui_state().unwrap();
    for (index, presentation) in [
        PresentationState {
            split_basis_points: 99,
            ..Default::default()
        },
        PresentationState {
            split_basis_points: 9901,
            ..Default::default()
        },
        PresentationState {
            task_height: 4001,
            ..Default::default()
        },
        PresentationState {
            current_movie: Some(String::new()),
            ..Default::default()
        },
        PresentationState {
            current_tv: Some("x".repeat(257)),
            ..Default::default()
        },
        PresentationState {
            current_tv: Some("bad\nitem".into()),
            ..Default::default()
        },
    ]
    .into_iter()
    .enumerate()
    {
        let mut candidate = original.clone();
        candidate.presentation = Some(presentation);
        assert!(store
            .save_ui_state(&format!("invalid-{index}"), candidate)
            .is_err());
        assert_eq!(store.ui_state().unwrap(), original);
    }
}

#[test]
fn selection_covers_hidden_pages_but_not_other_roots_spaces_or_search_results() {
    use tcm_core::{Configuration, LibraryRoot, Locale, ScanRequest};
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().canonicalize().unwrap();
    let mut roots = vec![];
    for (name, space) in [("a", Space::Movie), ("b", Space::Movie), ("tv", Space::Tv)] {
        let path = dir.join(name);
        std::fs::create_dir(&path).unwrap();
        let xml = if space == Space::Tv {
            "<tvshow><title>TV</title></tvshow>"
        } else {
            "<movie><title>Movie</title></movie>"
        };
        let count = if name == "a" { 105 } else { 1 };
        for index in 0..count {
            std::fs::write(path.join(format!("{index}.nfo")), xml).unwrap();
        }
        roots.push(LibraryRoot {
            id: name.into(),
            space,
            path: path.to_str().unwrap().into(),
        });
    }
    let store = Store::open(&dir.join("index.sqlite")).unwrap();
    store
        .configure(
            "roots",
            Configuration {
                revision: 0,
                locale: Locale::English,
                roots,
            },
        )
        .unwrap();
    for (id, space, ids) in [
        ("scan-movie", Space::Movie, vec!["a".into(), "b".into()]),
        ("scan-tv", Space::Tv, vec!["tv".into()]),
    ] {
        store
            .submit(ScanRequest {
                operation_id: id.into(),
                space,
                root_ids: ids,
            })
            .unwrap();
        store.run_next(|| false, |_| {}).unwrap();
    }
    let view = LibraryView {
        roots: vec!["a".into()],
        ..Default::default()
    };
    let page = store.browse(Space::Movie, view.clone()).unwrap();
    assert_eq!(page.total, 105);
    assert_eq!(page.items.len(), 100);
    let members = store.catalog_members(Space::Movie, view.clone()).unwrap();
    assert_eq!(members.len(), 105);
    assert_eq!(
        members,
        store
            .catalog_members(
                Space::Movie,
                LibraryView {
                    offset: 100,
                    ..view.clone()
                }
            )
            .unwrap()
    );
    assert_eq!(
        store
            .catalog_members(Space::Movie, LibraryView::default())
            .unwrap()
            .len(),
        106
    );
    assert!(store
        .catalog_members(Space::Tv, view.clone())
        .unwrap()
        .is_empty());
    assert!(store
        .catalog_members(
            Space::Movie,
            LibraryView {
                search: "absent".into(),
                ..view
            }
        )
        .unwrap()
        .is_empty());
    assert_eq!(
        store.tasks().unwrap().len(),
        2,
        "selection must not submit processing work"
    );
}
