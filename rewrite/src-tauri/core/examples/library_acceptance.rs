//! Read-only acceptance runner for an explicitly supplied sample library.
//! Its database is temporary and it never imports the production application state.
use serde_json::json;
use std::{collections::BTreeMap, path::Path, time::Instant};
use tcm_core::{store::Store, *};
fn query(space: Space, search: String) -> CatalogQuery {
    CatalogQuery {
        space,
        search,
        only_errors: false,
        offset: 0,
        limit: 500,
    }
}
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: library_acceptance MOVIE_ROOT TV_ROOT".into());
    }
    let temp = tempfile::tempdir()?;
    let db = temp.path().join("acceptance.sqlite");
    let store = Store::open(&db)?;
    let roots = args
        .iter()
        .zip([Space::Movie, Space::Tv])
        .enumerate()
        .map(|(i, (path, space))| LibraryRoot {
            id: format!("root-{i}"),
            space,
            path: path.clone(),
        })
        .collect();
    store.configure(
        "acceptance-config",
        Configuration {
            revision: 0,
            locale: Locale::Simplified,
            roots,
        },
    )?;
    let mut reports = vec![];
    for (i, space) in [Space::Movie, Space::Tv].into_iter().enumerate() {
        let started = Instant::now();
        let mut latest = vec![];
        let mut rounds = vec![];
        for pass in 0..2 {
            let id = format!("read-{i}-{pass}");
            store.submit(ScanRequest {
                operation_id: id.clone(),
                space: space.clone(),
                root_ids: vec![format!("root-{i}")],
            })?;
            store.run_next(|| false, |_| {})?;
            let task = store.task(&id)?;
            let page = store.query(query(space.clone(), String::new()))?;
            if page.total > 500 {
                return Err("Runner requires pagination extension for >500 items per space".into());
            }
            if pass == 1 {
                assert_eq!(
                    serde_json::to_value(&latest)?,
                    serde_json::to_value(&page.items)?
                );
            }
            rounds.push(task);
            latest = page.items;
        }
        let mut kinds = BTreeMap::<String, usize>::new();
        let mut imdb = BTreeMap::<String, usize>::new();
        for item in &latest {
            *kinds.entry(format!("{:?}", item.kind)).or_default() += 1;
            if !item.imdb.is_empty() {
                *imdb.entry(item.imdb.clone()).or_default() += 1;
            }
            assert_eq!(
                serde_json::to_value(store.item(&item.id)?)?,
                serde_json::to_value(item)?
            );
            let filename = Path::new(&item.path)
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_owned();
            assert!(store
                .query(query(space.clone(), filename))?
                .items
                .iter()
                .any(|found| found.id == item.id));
        }
        reports.push(json!({"space":space,"items":latest,"rounds":rounds,"kinds":kinds,"repeated_imdb":imdb.into_iter().filter(|(_,n)|*n>1).collect::<BTreeMap<_,_>>(),"elapsed_ms":started.elapsed().as_millis(),"unchanged_refresh":"passed","inspector_and_path_search":"passed"}));
    }
    drop(store);
    let reopened = Store::open(&db)?;
    assert_eq!(reopened.tasks()?.len(), 4);
    println!(
        "{}",
        serde_json::to_string(
            &json!({"spaces":reports,"history_reopen":"passed","state":"temporary-database","media_access":"read-only"})
        )?
    );
    Ok(())
}
