//! CI driver calls the same Store, Integration and CardService as the desktop.
//! It only accepts explicitly supplied temporary paths; it never discovers production data.
use std::{
    io::{self, BufRead, Write},
    path::Path,
    sync::Arc,
};
use tcm_core::{
    card_service::CardService,
    emby::{self, Integration},
    store::Store,
    *,
};
fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        return Err(
            "usage: emby_acceptance WEB_ROOT PRIVATE_ROOT MOVIE_ROOT EMBY_DATA_ROOT".into(),
        );
    }
    let discovered = tcm_core::emby_libraries::discover(Path::new(&args[3]), "4.9.5.0", &[])?;
    let movie = Path::new(&args[2]).canonicalize()?;
    if !discovered.iter().any(|root| {
        root.spaces.contains(&Space::Movie)
            && root
                .local_path
                .as_ref()
                .is_some_and(|p| Path::new(p).canonicalize().is_ok_and(|p| p == movie))
    }) {
        return Err("Real Emby database did not expose the physical movie root".into());
    }
    let private = Path::new(&args[1]);
    std::fs::create_dir_all(private)?;
    let store = Arc::new(Store::open(&private.join("library.sqlite"))?);
    store.configure(
        "roots",
        Configuration {
            revision: 0,
            locale: Locale::English,
            roots: vec![LibraryRoot {
                id: "movie".into(),
                space: Space::Movie,
                path: args[2].clone(),
            }],
        },
    )?;
    store.submit(ScanRequest {
        operation_id: "scan".into(),
        space: Space::Movie,
        root_ids: vec!["movie".into()],
    })?;
    store.run_next(|| false, |_| {})?;
    if store.task("scan")?.state != TaskState::Completed {
        return Err("NFO scan did not complete".into());
    }
    let index = emby::public_index(&store.all_items()?, emby::timestamp());
    if index.items.is_empty() {
        return Err("No public card data".into());
    }
    let integration = Arc::new(Integration::open(
        Path::new(&args[0]),
        &private.join("backups"),
    )?);
    let js = include_bytes!("../../../web-card/technical-specs-card.js");
    let plan = integration.plan(
        "install",
        "install",
        js,
        &index,
        &emby::bundled_card_languages()?,
    )?;
    integration.apply(&plan.id, &plan.fingerprint)?;
    let mut service =
        CardService::start_with_store(integration.clone(), "ci-session", store.clone())?;
    println!(
        "{}",
        serde_json::json!({"phase":"running","items":index.items.len(),"physical_root_discovery":true})
    );
    io::stdout().flush()?;
    for line in io::stdin().lock().lines() {
        match line?.as_str() {
            "stop" => {
                println!(
                    "{}",
                    serde_json::to_string(&service.stop().map_err(|e| format!("stop: {e:?}"))?)?
                );
            }
            "start" => {
                service = CardService::start_with_store(
                    integration.clone(),
                    "ci-session-restarted",
                    store.clone(),
                )?;
                println!("{}", serde_json::to_string(&service.status()?)?);
            }
            "remove" => {
                service.stop()?;
                let p = integration.plan(
                    "remove",
                    "remove",
                    js,
                    &index,
                    &emby::bundled_card_languages()?,
                )?;
                integration.apply(&p.id, &p.fingerprint)?;
                println!("{}", serde_json::json!({"phase":"removed"}));
                io::stdout().flush()?;
                break;
            }
            _ => return Err("Unknown acceptance command".into()),
        }
        io::stdout().flush()?;
    }
    service.stop()?;
    Ok(())
}
