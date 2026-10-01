use tcm_core::{store::Store, ui::*, *};
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
