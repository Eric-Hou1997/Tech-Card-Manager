use crate::desktop::Desktop;
use product_core::{
    languages::{self as core, Catalog, LanguageSnapshot},
    AppError, Locale, Result,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
    time::Duration,
};
use tauri::{Emitter, Manager, State};
#[derive(Default)]
pub struct Languages {
    busy: tauri::async_runtime::Mutex<()>,
    downloading: Mutex<BTreeSet<String>>,
    restore_lock: tauri::async_runtime::Mutex<()>,
    restore_attempts: Mutex<BTreeSet<String>>,
    errors: Mutex<BTreeMap<String, String>>,
}
pub struct LanguageMenus(
    pub Vec<tauri::menu::MenuItem<tauri::Wry>>,
    pub Vec<tauri::menu::MenuItem<tauri::Wry>>,
);
struct DownloadFlag<'a> {
    values: &'a Mutex<BTreeSet<String>>,
    code: String,
}
impl<'a> DownloadFlag<'a> {
    fn begin(values: &'a Mutex<BTreeSet<String>>, code: &str) -> Result<Self> {
        if !values.lock().map_err(error)?.insert(code.into()) {
            return Err(AppError::new("language-downloading", "该语言包正在下载"));
        }
        Ok(Self {
            values,
            code: code.into(),
        })
    }
}
impl Drop for DownloadFlag<'_> {
    fn drop(&mut self) {
        if let Ok(mut values) = self.values.lock() {
            values.remove(&self.code);
        }
    }
}
fn error(message: impl ToString) -> AppError {
    AppError::new("language-install", message)
}
pub(crate) fn snapshot(app: &tauri::AppHandle, state: &Languages) -> Result<LanguageSnapshot> {
    let catalog = Catalog::embedded()?;
    let desktop = app.state::<Desktop>();
    let locale = desktop.store.configuration()?.locale;
    let mut options = core::options();
    let mut web_messages = BTreeMap::new();
    let mut native_messages = BTreeMap::new();
    let downloading = state.downloading.lock().map_err(error)?.clone();
    let errors = state.errors.lock().map_err(error)?;
    for option in options.iter_mut().filter(|o| !o.built_in) {
        match desktop.store.legacy_language_pack(&catalog, &option.code) {
            Ok(Some(sections)) => {
                option.installed = true;
                option.state = "installed".into();
                if option.code == locale.as_str() {
                    web_messages = sections["web"].clone();
                    native_messages = sections["native"].clone();
                }
            }
            Ok(None) => {}
            Err(e) => {
                option.state = "failed".into();
                option.error = Some(e.message);
            }
        }
        if downloading.contains(&option.code) {
            option.state = "downloading".into();
        } else if !option.installed {
            if let Some(message) = errors.get(&option.code) {
                option.state = "failed".into();
                option.error = Some(message.clone());
            }
        }
    }
    Ok(LanguageSnapshot {
        locale,
        options,
        web_messages,
        native_messages,
    })
}
fn stable_id(text: &str) -> String {
    let mut hash = 14695981039346656037u64;
    for byte in text.trim().bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    format!("legacy.{hash:016x}")
}
fn presentation_text(
    snapshot: &LanguageSnapshot,
    chinese: &str,
    english: &str,
    messages: &BTreeMap<String, String>,
) -> Result<String> {
    match snapshot.locale {
        Locale::Simplified => Ok(chinese.into()),
        Locale::Traditional => {
            let baseline: serde_json::Value =
                serde_json::from_str(include_str!("../../src/assets/baseline-languages.json"))?;
            let pairs: Vec<(String, String)> =
                serde_json::from_value(baseline["traditional"].clone())?;
            Ok(pairs
                .into_iter()
                .fold(chinese.to_owned(), |text, (from, to)| {
                    text.replace(&from, &to)
                }))
        }
        _ => Ok(messages
            .get(&stable_id(english))
            .cloned()
            .unwrap_or_else(|| english.into())),
    }
}
pub(crate) fn web_message(snapshot: &LanguageSnapshot, source: &str) -> Result<String> {
    let baseline: serde_json::Value =
        serde_json::from_str(include_str!("../../src/assets/baseline-languages.json"))?;
    let english = baseline["english"][source].as_str().ok_or_else(|| {
        AppError::new(
            "language-message",
            "Original confirmation message is missing",
        )
    })?;
    presentation_text(snapshot, source, english, &snapshot.web_messages)
}
pub(crate) fn native_message(
    snapshot: &LanguageSnapshot,
    chinese: &str,
    english: &str,
) -> Result<String> {
    presentation_text(snapshot, chinese, english, &snapshot.native_messages)
}
pub(crate) fn backend_message(app: &tauri::AppHandle, source: &str) -> String {
    app.try_state::<Desktop>()
        .and_then(|desktop| desktop.store.localize_backend_text(source).ok())
        .unwrap_or_else(|| source.into())
}
pub fn synchronize_menus(app: &tauri::AppHandle, snapshot: &LanguageSnapshot) -> Result<()> {
    let Some(menus) = app.try_state::<LanguageMenus>() else {
        return Ok(());
    };
    let labels = menu_labels(snapshot);
    for item in &menus.0 {
        item.set_text(&labels.0).map_err(error)?;
    }
    for item in &menus.1 {
        item.set_text(&labels.1).map_err(error)?;
    }
    Ok(())
}
fn menu_labels(snapshot: &LanguageSnapshot) -> (String, String) {
    match snapshot.locale {
        Locale::Simplified => (
            "打开 Tech Card Manager".into(),
            "退出 Tech Card Manager".into(),
        ),
        Locale::Traditional => (
            "打开 Tech Card Manager".into(),
            "退出 Tech Card Manager".into(),
        ),
        _ => {
            let value = |s: &str| {
                snapshot
                    .native_messages
                    .get(&stable_id(s))
                    .cloned()
                    .unwrap_or_else(|| s.into())
            };
            (
                value("Open Tech Card Manager"),
                value("Exit Tech Card Manager"),
            )
        }
    }
}
#[tauri::command]
pub async fn language_status(app: tauri::AppHandle) -> Result<LanguageSnapshot> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Languages>();
        let result = snapshot(&app, &state)?;
        synchronize_menus(&app, &result)?;
        Ok(result)
    })
    .await
    .map_err(error)?
}
#[tauri::command]
pub async fn choose_language(
    locale: Locale,
    app: tauri::AppHandle,
    state: State<'_, Languages>,
) -> Result<LanguageSnapshot> {
    let _guard = state
        .busy
        .try_lock()
        .map_err(|_| error("该语言包正在下载"))?;
    let catalog = Catalog::embedded()?;
    let code = locale.as_str();
    let external = catalog.languages.contains_key(code);
    if external {
        let desktop = app.state::<Desktop>();
        ensure_pack(
            &desktop.store,
            &catalog,
            code,
            &state,
            download(&catalog, code),
            || app.emit("language-state-changed", ()).map_err(error),
        )
        .await?;
    }
    let config = app
        .state::<Desktop>()
        .store
        .select_ui_language(&catalog, locale)?;
    let result = snapshot(&app, &state)?;
    synchronize_menus(&app, &result)?;
    app.emit("configuration-changed", config).map_err(error)?;
    Ok(result)
}

async fn ensure_pack(
    store: &product_core::store::Store,
    catalog: &Catalog,
    code: &str,
    state: &Languages,
    transfer: impl std::future::Future<Output = Result<Vec<u8>>>,
    notify: impl Fn() -> Result<()>,
) -> Result<()> {
    if store
        .current_language_pack(catalog, code)
        .is_ok_and(|p| p.is_some())
    {
        return Ok(());
    }
    let flag = DownloadFlag::begin(&state.downloading, code)?;
    notify()?;
    let result = transfer.await.and_then(|bytes| {
        store
            .install_legacy_language_pack(catalog, code, &bytes)
            .map(|_| ())
    });
    drop(flag);
    match &result {
        Ok(()) => {
            state.errors.lock().map_err(error)?.remove(code);
        }
        Err(failure) => {
            state
                .errors
                .lock()
                .map_err(error)?
                .insert(code.into(), failure.message.clone());
        }
    }
    notify()?;
    result
}
#[tauri::command]
pub async fn restore_language_packs(
    app: tauri::AppHandle,
    state: State<'_, Languages>,
) -> Result<LanguageSnapshot> {
    let Ok(_guard) = state.restore_lock.try_lock() else {
        return language_status(app).await;
    };
    let catalog = Catalog::embedded()?;
    let store = app.state::<Desktop>().store.clone();
    for code in store.language_restore_candidates(&catalog)? {
        if state.downloading.lock().map_err(error)?.contains(&code) {
            continue;
        }
        let key = format!("{code}:{}", catalog.descriptor(&code)?.sha256);
        if !state
            .restore_attempts
            .lock()
            .map_err(error)?
            .insert(key.clone())
        {
            continue;
        }
        if let Err(failure) = ensure_pack(
            &store,
            &catalog,
            &code,
            &state,
            download(&catalog, &code),
            || app.emit("language-state-changed", ()).map_err(error),
        )
        .await
        {
            if failure.code == "language-downloading" {
                state.restore_attempts.lock().map_err(error)?.remove(&key);
            } else {
                eprintln!("language-pack-restore: {code}: {failure}");
            }
        }
    }
    language_status(app).await
}

async fn download(catalog: &Catalog, locale: &str) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .https_only(true)
        .timeout(Duration::from_secs(90))
        .user_agent(concat!("Tech-Card-Manager/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 10
                || !product_core::manual_update::official_redirect(attempt.url().as_str())
            {
                attempt.error("语言包下载重定向到非官方地址或次数过多，已拒绝")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(error)?;
    let mut response = client
        .get(catalog.url(locale)?)
        .send()
        .await
        .map_err(|e| error(format!("语言包下载失败：{e}")))?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(error(format!(
            "语言包下载失败（HTTP {}）",
            response.status().as_u16()
        )));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(error)? {
        if bytes.len() + chunk.len() > core::MAX_BYTES {
            return Err(error("语言包过大，已拒绝安装"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restoring_a_pack_does_not_block_builtin_selection_and_cancellation_releases_only_its_download(
    ) {
        use std::future::Future;
        let temp = tempfile::tempdir().unwrap();
        let store = product_core::store::Store::open(&temp.path().join("state.sqlite")).unwrap();
        let catalog = Catalog::embedded().unwrap();
        let state = Languages::default();
        let _restoring = state.restore_lock.try_lock().unwrap();
        let mut transfer = Box::pin(ensure_pack(
            &store,
            &catalog,
            "fr-FR",
            &state,
            std::future::pending(),
            || Ok(()),
        ));
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(transfer.as_mut().poll(&mut cx).is_pending());
        assert!(state.downloading.lock().unwrap().contains("fr-FR"));
        assert!(DownloadFlag::begin(&state.downloading, "fr-FR").is_err());
        let other = DownloadFlag::begin(&state.downloading, "ja-JP").unwrap();
        let _selection = state.busy.try_lock().unwrap();
        store.select_ui_language(&catalog, Locale::English).unwrap();
        assert_eq!(store.configuration().unwrap().locale, Locale::English);
        drop(transfer);
        assert_eq!(
            *state.downloading.lock().unwrap(),
            BTreeSet::from(["ja-JP".into()])
        );
        drop(other);
        assert!(state.downloading.lock().unwrap().is_empty());
    }
    #[test]
    fn failed_restore_reports_the_error_and_never_selects_or_installs_the_language() {
        tauri::async_runtime::block_on(async {
            let temp = tempfile::tempdir().unwrap();
            let store =
                product_core::store::Store::open(&temp.path().join("state.sqlite")).unwrap();
            let catalog = Catalog::embedded().unwrap();
            let state = Languages::default();
            let phases = Mutex::new(Vec::new());
            let result = ensure_pack(
                &store,
                &catalog,
                "fr-FR",
                &state,
                async { Err(error("offline fixture")) },
                || {
                    phases
                        .lock()
                        .unwrap()
                        .push(state.downloading.lock().unwrap().contains("fr-FR"));
                    Ok(())
                },
            )
            .await;
            assert!(result.is_err());
            assert_eq!(*phases.lock().unwrap(), vec![true, false]);
            assert_eq!(state.errors.lock().unwrap()["fr-FR"], "offline fixture");
            assert_eq!(store.configuration().unwrap().locale, Locale::Simplified);
            assert!(store
                .legacy_language_pack(&catalog, "fr-FR")
                .unwrap()
                .is_none());
        });
    }
    #[test]
    fn native_menu_text_uses_the_original_sources_and_the_selected_verified_dictionary() {
        let baseline: serde_json::Value =
            serde_json::from_str(include_str!("../../src/assets/baseline-languages.json")).unwrap();
        let confirmations: Vec<_> = baseline["english"]
            .as_object()
            .unwrap()
            .iter()
            .filter(|(text, _)| text.starts_with("这会") || text.starts_with("保存后，"))
            .map(|(zh, en)| (zh.as_str(), en.as_str().unwrap()))
            .collect();
        assert_eq!(confirmations.len(), 4);
        let mut snapshot = LanguageSnapshot {
            locale: Locale::Simplified,
            options: core::options(),
            web_messages: BTreeMap::new(),
            native_messages: BTreeMap::new(),
        };
        for (zh, _) in &confirmations {
            assert_eq!(web_message(&snapshot, zh).unwrap(), *zh);
        }
        assert_eq!(
            menu_labels(&snapshot),
            (
                "打开 Tech Card Manager".into(),
                "退出 Tech Card Manager".into()
            )
        );
        snapshot.locale = Locale::Traditional;
        assert_eq!(
            native_message(
                &snapshot,
                "选择电影或电视剧媒体目录",
                "Select a Movie or TV Show media folder"
            )
            .unwrap(),
            "選擇電影或電視劇媒体目錄"
        );
        for (zh, _) in &confirmations {
            let text = web_message(&snapshot, zh).unwrap();
            assert_eq!(text.contains("NFO"), zh.contains("NFO"));
            assert!(!text.contains("读取"));
        }
        assert_eq!(
            menu_labels(&snapshot),
            (
                "打开 Tech Card Manager".into(),
                "退出 Tech Card Manager".into()
            )
        );
        snapshot.locale = Locale::English;
        for (zh, en) in &confirmations {
            assert_eq!(web_message(&snapshot, zh).unwrap(), *en);
        }
        assert_eq!(
            menu_labels(&snapshot),
            (
                "Open Tech Card Manager".into(),
                "Exit Tech Card Manager".into()
            )
        );
        for (locale, source) in [
            (
                Locale::French,
                include_str!("../../../language-packs/fr-FR/r1/translations.json"),
            ),
            (
                Locale::Russian,
                include_str!("../../../language-packs/ru-RU/r1/translations.json"),
            ),
            (
                Locale::Japanese,
                include_str!("../../../language-packs/ja-JP/r1/translations.json"),
            ),
            (
                Locale::Spanish,
                include_str!("../../../language-packs/es-ES/r1/translations.json"),
            ),
            (
                Locale::Thai,
                include_str!("../../../language-packs/th-TH/r1/translations.json"),
            ),
        ] {
            let source: BTreeMap<String, BTreeMap<String, String>> =
                serde_json::from_str(source).unwrap();
            snapshot.locale = locale;
            snapshot.native_messages = source["native"]
                .iter()
                .map(|(k, v)| (stable_id(k), v.clone()))
                .collect();
            snapshot.web_messages = source["web"]
                .iter()
                .map(|(k, v)| (stable_id(k), v.clone()))
                .collect();
            for (zh, en) in &confirmations {
                assert_eq!(web_message(&snapshot, zh).unwrap(), source["web"][*en]);
            }
            assert_eq!(
                native_message(
                    &snapshot,
                    "选择电影或电视剧媒体目录",
                    "Select a Movie or TV Show media folder"
                )
                .unwrap(),
                source["native"]["Select a Movie or TV Show media folder"]
            );
            assert_eq!(
                menu_labels(&snapshot),
                (
                    source["native"]["Open Tech Card Manager"].clone(),
                    source["native"]["Exit Tech Card Manager"].clone()
                )
            );
        }
    }
    #[test]
    fn download_state_has_one_owner_and_is_cleared_on_early_return() {
        let value = Mutex::new(BTreeSet::new());
        let failure = || -> Result<()> {
            let _flag = DownloadFlag::begin(&value, "fr-FR")?;
            Err(error("download failed"))
        };
        assert!(failure().is_err());
        assert!(value.lock().unwrap().is_empty());
    }
}
