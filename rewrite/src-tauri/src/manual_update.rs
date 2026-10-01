use product_core::manual_update::{
    self as core, Asset, Cache, Failure, Release, ReleaseCheck, Result,
};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use tauri::{Manager, State};

#[derive(Default)]
struct Session {
    loaded: bool,
    state: core::State,
    last: Option<Result<ReleaseCheck>>,
}
#[derive(Default)]
pub struct ManualUpdates {
    session: tauri::async_runtime::Mutex<Session>,
    completed: AtomicU64,
}

fn network(error: reqwest::Error) -> Failure {
    if error.to_string().to_lowercase().contains("proxy") {
        Failure::new(
            "proxy-connection-failed",
            format!("无法连接代理服务器：{error}"),
        )
    } else {
        Failure::new("network-unavailable", format!("无法连接 GitHub：{error}"))
    }
}
struct Response {
    status: u16,
    headers: BTreeMap<String, String>,
    url: String,
    body: Vec<u8>,
}
trait Transport {
    async fn request(&self, url: &str, etag: &str, head: bool, html: bool) -> Result<Response>;
}
struct GitHub(reqwest::Client);
impl GitHub {
    fn new() -> Result<Self> {
        reqwest::Client::builder()
            .https_only(true)
            .timeout(Duration::from_secs(20))
            .user_agent(concat!("Tech-Card-Manager/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 10
                    || !core::official_redirect(attempt.url().as_str())
                {
                    attempt.error("GitHub 更新重定向到非官方地址或次数过多，已拒绝")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map(Self)
            .map_err(network)
    }
}
impl Transport for GitHub {
    async fn request(&self, url: &str, etag: &str, head: bool, html: bool) -> Result<Response> {
        let mut request = if head {
            self.0.head(url)
        } else {
            self.0.get(url)
        }
        .header(
            "Accept",
            if html {
                "text/html"
            } else {
                "application/vnd.github+json"
            },
        )
        .header("X-GitHub-Api-Version", "2022-11-28");
        if !etag.is_empty() {
            request = request.header("If-None-Match", etag);
        }
        let mut response = request.send().await.map_err(network)?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.to_string(), v.into())))
            .collect();
        let url = response.url().to_string();
        let limit = if status == 200 {
            2 * 1024 * 1024
        } else {
            64 * 1024
        };
        let mut body = Vec::new();
        if !head {
            while let Some(chunk) = response.chunk().await.map_err(network)? {
                if body.len() + chunk.len() > limit {
                    if status == 200 {
                        return Err(Failure::new(
                            "invalid-release-response",
                            "GitHub 更新信息超过允许大小",
                        ));
                    }
                    body.extend_from_slice(&chunk[..limit - body.len()]);
                    break;
                }
                body.extend_from_slice(&chunk);
            }
        }
        Ok(Response {
            status,
            headers,
            url,
            body,
        })
    }
}
async fn api(
    transport: &impl Transport,
    pattern: Option<&str>,
    prior: Option<&Cache>,
    now: i64,
) -> Result<(Release, String)> {
    let response = transport
        .request(
            core::API,
            prior.map(|c| c.etag.as_str()).unwrap_or(""),
            false,
            false,
        )
        .await?;
    if response.status == 304 {
        return prior
            .map(|c| (c.release.clone(), c.etag.clone()))
            .ok_or_else(|| {
                Failure::new("invalid-not-modified", "GitHub 返回了无法使用的未修改状态")
            });
    }
    if response.status != 200 {
        return Err(core::http_failure(
            response.status,
            &response.headers,
            &response.body,
            now,
        ));
    }
    let release = serde_json::from_slice(&response.body).map_err(|e| {
        Failure::new(
            "invalid-release-response",
            format!("GitHub 更新信息无效：{e}"),
        )
    })?;
    core::validate_release(&release)?;
    if let Some(pattern) = pattern {
        core::select(&release, pattern)?;
    }
    Ok((
        release,
        response.headers.get("etag").cloned().unwrap_or_default(),
    ))
}
async fn page(transport: &impl Transport, pattern: Option<&str>, now: i64) -> Result<Release> {
    let response = transport
        .request(
            &format!("{}/releases/latest", core::REPOSITORY),
            "",
            false,
            true,
        )
        .await?;
    if response.status != 200 {
        return Err(core::http_failure(
            response.status,
            &response.headers,
            &response.body,
            now,
        ));
    }
    let tag = core::release_tag(&response.url)?;
    let assets = if let Some(pattern) = pattern {
        let name = core::package_name(pattern, &tag)?;
        let url = core::asset_url(&tag, &name);
        let asset = transport.request(&url, "", true, false).await?;
        if !matches!(asset.status, 200 | 206) {
            return Err(core::http_failure(
                asset.status,
                &asset.headers,
                &asset.body,
                now,
            ));
        }
        vec![Asset {
            name,
            browser_download_url: url,
        }]
    } else {
        Vec::new()
    };
    Ok(Release {
        tag_name: tag,
        html_url: response.url,
        draft: false,
        prerelease: false,
        published_at: String::new(),
        assets,
    })
}
async fn fetch(
    transport: &impl Transport,
    pattern: Option<&str>,
    prior: Option<&Cache>,
    now: i64,
) -> Result<(Release, String, &'static str)> {
    match api(transport, pattern, prior, now).await {
        Ok((release, etag)) => Ok((release, etag, "github-api")),
        Err(failure)
            if matches!(
                failure.code.as_str(),
                "invalid-not-modified" | "update-asset-mismatch"
            ) =>
        {
            Err(failure)
        }
        Err(failure) => match page(transport, pattern, now).await {
            Ok(release) => Ok((release, String::new(), "github-release-page")),
            Err(fallback)
                if matches!(
                    failure.code.as_str(),
                    "network-unavailable" | "github-unavailable" | "github-http-error"
                ) =>
            {
                Err(fallback)
            }
            Err(_) => Err(failure),
        },
    }
}

fn package_pattern(app: &tauri::AppHandle) -> Result<&'static str> {
    let identity = crate::update::update_identity(app.clone())
        .map_err(|e| Failure::new(&e.code, e.message))?;
    if product_core::OFFICIAL_RELEASE {
        return core::release_pattern(&identity);
    }
    core::installation_pattern(
        &identity,
        option_env!("TCM_RELEASE_PACKAGE_TARGET"),
        option_env!("TCM_RELEASE_PACKAGE_PATTERN"),
    )
}
fn optional_package_pattern(app: &tauri::AppHandle) -> Result<Option<&'static str>> {
    match package_pattern(app) {
        Ok(pattern) => Ok(Some(pattern)),
        Err(failure) if failure.code == "update-package-not-configured" => Ok(None),
        Err(failure) => Err(failure),
    }
}
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn candidate_url(last: Option<&Result<ReleaseCheck>>, id: &str) -> Result<String> {
    let candidate = last
        .and_then(|r| r.as_ref().ok())
        .filter(|r| r.available && r.candidate_id == id)
        .ok_or_else(|| {
            Failure::new(
                "update-candidate-stale",
                "安装包地址尚未确认，请重新检查更新",
            )
        })?;
    Ok(candidate.package_url.clone())
}

#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProductLink {
    Privacy,
    Terms,
    License,
    Repository,
}

fn product_link(kind: ProductLink, english: bool) -> &'static str {
    match (kind, english) {
        (ProductLink::Privacy, false) => {
            "https://github.com/Eric-Hou1997/Tech-Card-Manager/blob/main/PRIVACY.md"
        }
        (ProductLink::Privacy, true) => {
            "https://github.com/Eric-Hou1997/Tech-Card-Manager/blob/main/docs/legal/PRIVACY.en.md"
        }
        (ProductLink::Terms, false) => {
            "https://github.com/Eric-Hou1997/Tech-Card-Manager/blob/main/TERMS.md"
        }
        (ProductLink::Terms, true) => {
            "https://github.com/Eric-Hou1997/Tech-Card-Manager/blob/main/docs/legal/TERMS.en.md"
        }
        (ProductLink::License, _) => {
            "https://github.com/Eric-Hou1997/Tech-Card-Manager/blob/main/LICENSE"
        }
        (ProductLink::Repository, _) => "https://github.com/Eric-Hou1997/Tech-Card-Manager",
    }
}

fn open_external(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let status = std::process::Command::new("/usr/bin/open")
        .arg(url)
        .status();
    #[cfg(windows)]
    let status = std::process::Command::new("rundll32.exe")
        .arg("url.dll,FileProtocolHandler")
        .arg(url)
        .status();
    #[cfg(target_os = "linux")]
    let status = std::process::Command::new("xdg-open").arg(url).status();
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(Failure::new(
            "browser-open",
            format!("无法打开浏览器：{status}"),
        )),
        Err(error) => Err(Failure::new(
            "browser-open",
            format!("无法打开浏览器：{error}"),
        )),
    }
}

#[tauri::command]
pub async fn open_product_link(kind: ProductLink, english: bool) -> Result<()> {
    let url = product_link(kind, english);
    tauri::async_runtime::spawn_blocking(move || open_external(url))
        .await
        .map_err(|error| Failure::new("browser-open", error))?
}

#[tauri::command]
pub async fn check_card_update(
    force: bool,
    app: tauri::AppHandle,
    updates: State<'_, ManualUpdates>,
) -> Result<ReleaseCheck> {
    let revision = updates.completed.load(Ordering::SeqCst);
    let mut session = updates.session.lock().await;
    if revision != updates.completed.load(Ordering::SeqCst) {
        if let Some(last) = &session.last {
            return last.clone();
        }
    }
    let result = check_locked(force, &app, &mut session).await;
    session.last = Some(result.clone());
    updates.completed.fetch_add(1, Ordering::SeqCst);
    result
}
async fn check_locked(
    force: bool,
    app: &tauri::AppHandle,
    session: &mut Session,
) -> Result<ReleaseCheck> {
    let pattern = optional_package_pattern(app)?;
    let path = app
        .path()
        .app_data_dir()
        .map_err(|e| Failure::new("update-cache-path", e))?
        .join("update-state.json");
    let directory = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let checked = now();
    if !session.loaded {
        session.loaded = true;
        session.state.cache = match pattern {
            Some(pattern) => core::load_cache(&path, pattern, checked),
            None => core::load_metadata_cache(&path, checked),
        };
    }
    if let Some(cached) = session.state.cached(force, checked) {
        let (cache, source, warning) = cached?;
        return match pattern {
            Some(pattern) => core::result(
                cache,
                pattern,
                env!("CARGO_PKG_VERSION"),
                &directory,
                source,
                warning,
            ),
            None => core::result_without_package(
                cache,
                env!("CARGO_PKG_VERSION"),
                &directory,
                source,
                warning,
            ),
        };
    }
    let fetched = match GitHub::new() {
        Ok(client) => fetch(&client, pattern, session.state.cache.as_ref(), checked).await,
        Err(error) => Err(error),
    };
    match fetched {
        Ok((release, etag, source)) => {
            let cache = Cache {
                schema_version: 1,
                etag,
                checked_at: core::time(checked),
                release,
            };
            let warning = core::save_cache(&path, &cache).err();
            let result = match pattern {
                Some(pattern) => core::result(
                    &cache,
                    pattern,
                    env!("CARGO_PKG_VERSION"),
                    &directory,
                    source,
                    warning.as_ref(),
                ),
                None => core::result_without_package(
                    &cache,
                    env!("CARGO_PKG_VERSION"),
                    &directory,
                    source,
                    warning.as_ref(),
                ),
            };
            session.state.cache = Some(cache);
            session.state.cooldown = None;
            result
        }
        Err(failure) => {
            let (cache, source, warning) = session.state.failed(failure, checked)?;
            match pattern {
                Some(pattern) => core::result(
                    cache,
                    pattern,
                    env!("CARGO_PKG_VERSION"),
                    &directory,
                    source,
                    warning,
                ),
                None => core::result_without_package(
                    cache,
                    env!("CARGO_PKG_VERSION"),
                    &directory,
                    source,
                    warning,
                ),
            }
        }
    }
}

#[tauri::command]
pub async fn open_card_update(
    candidate_id: String,
    updates: State<'_, ManualUpdates>,
) -> Result<()> {
    let session = updates.session.lock().await;
    let url = candidate_url(session.last.as_ref(), &candidate_id)?;
    // The URL is only from the last validated release, never a renderer supplied URL.
    tauri::async_runtime::spawn_blocking(move || {
        open_external(&url).map_err(|error| {
            Failure::new(
                "update-browser-open",
                error
                    .message
                    .replace("无法打开浏览器", "无法打开安装包下载地址"),
            )
        })
    })
    .await
    .map_err(|e| Failure::new("update-browser-open", e))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::VecDeque, sync::Mutex};
    const PATTERN: &str = "TCM-v{version}-Windows-x64-EXE.zip";
    const NOW: i64 = 1_789_257_600;
    struct Fake {
        replies: Mutex<VecDeque<Response>>,
        calls: Mutex<Vec<(String, String, bool, bool)>>,
    }
    impl Fake {
        fn new(replies: Vec<Response>) -> Self {
            Self {
                replies: Mutex::new(replies.into()),
                calls: Mutex::new(Vec::new()),
            }
        }
    }
    impl Transport for Fake {
        async fn request(&self, url: &str, etag: &str, head: bool, html: bool) -> Result<Response> {
            self.calls
                .lock()
                .unwrap()
                .push((url.into(), etag.into(), head, html));
            Ok(self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected request"))
        }
    }
    fn response(status: u16, url: &str, body: &[u8]) -> Response {
        Response {
            status,
            url: url.into(),
            headers: BTreeMap::new(),
            body: body.into(),
        }
    }
    fn cache() -> Cache {
        let tag = "v5.0.1";
        let name = core::package_name(PATTERN, tag).unwrap();
        Cache {
            schema_version: 1,
            etag: "previous-etag".into(),
            checked_at: core::time(NOW),
            release: Release {
                tag_name: tag.into(),
                html_url: format!("{}/releases/tag/{tag}", core::REPOSITORY),
                draft: false,
                prerelease: false,
                published_at: String::new(),
                assets: vec![Asset {
                    browser_download_url: core::asset_url(tag, &name),
                    name,
                }],
            },
        }
    }
    #[test]
    fn product_links_are_fixed_and_language_selection_only_changes_legal_documents() {
        assert_eq!(
            product_link(ProductLink::Privacy, false),
            "https://github.com/Eric-Hou1997/Tech-Card-Manager/blob/main/PRIVACY.md"
        );
        assert_eq!(
            product_link(ProductLink::Privacy, true),
            "https://github.com/Eric-Hou1997/Tech-Card-Manager/blob/main/docs/legal/PRIVACY.en.md"
        );
        assert_eq!(
            product_link(ProductLink::Terms, true),
            "https://github.com/Eric-Hou1997/Tech-Card-Manager/blob/main/docs/legal/TERMS.en.md"
        );
        assert_eq!(
            product_link(ProductLink::License, true),
            "https://github.com/Eric-Hou1997/Tech-Card-Manager/blob/main/LICENSE"
        );
        assert_eq!(
            product_link(ProductLink::Repository, false),
            "https://github.com/Eric-Hou1997/Tech-Card-Manager"
        );
    }
    #[test]
    fn not_modified_reuses_only_a_real_prior_release_without_page_requests() {
        tauri::async_runtime::block_on(async {
            let prior = cache();
            let transport = Fake::new(vec![response(304, core::API, b"")]);
            let (release, etag, source) = fetch(&transport, Some(PATTERN), Some(&prior), NOW)
                .await
                .unwrap();
            assert_eq!(release, prior.release);
            assert_eq!(etag, prior.etag);
            assert_eq!(source, "github-api");
            assert_eq!(transport.calls.lock().unwrap()[0].1, "previous-etag");
            let transport = Fake::new(vec![response(304, core::API, b"")]);
            assert_eq!(
                fetch(&transport, Some(PATTERN), None, NOW)
                    .await
                    .unwrap_err()
                    .code,
                "invalid-not-modified"
            );
            assert_eq!(transport.calls.lock().unwrap().len(), 1);
        });
    }
    #[test]
    fn api_failure_falls_back_to_official_tag_and_exact_asset_head_probe() {
        tauri::async_runtime::block_on(async {
            let prior = cache();
            let package = &prior.release.assets[0].browser_download_url;
            let transport = Fake::new(vec![
                response(503, core::API, b""),
                response(200, &prior.release.html_url, b"html"),
                response(206, package, b""),
            ]);
            let (release, etag, source) =
                fetch(&transport, Some(PATTERN), None, NOW).await.unwrap();
            assert_eq!(source, "github-release-page");
            assert!(etag.is_empty());
            assert_eq!(release.assets, prior.release.assets);
            let calls = transport.calls.lock().unwrap();
            assert_eq!(calls.len(), 3);
            assert!(calls[1].3);
            assert_eq!(&calls[2].0, package);
            assert!(calls[2].2);
        });
    }
    #[test]
    fn failed_fallback_preserves_primary_limit_and_rejects_unverified_asset() {
        tauri::async_runtime::block_on(async {
            let mut primary = response(403, core::API, b"");
            primary
                .headers
                .insert("x-ratelimit-remaining".into(), "0".into());
            let transport = Fake::new(vec![primary, response(503, "", b"")]);
            let failure = fetch(&transport, Some(PATTERN), None, NOW)
                .await
                .unwrap_err();
            assert_eq!(failure.code, "github-primary-rate-limit");
            assert_eq!(failure.retry_time(), Some(NOW + 60));
            let prior = cache();
            let transport = Fake::new(vec![
                response(503, core::API, b""),
                response(200, &prior.release.html_url, b""),
                response(404, "", b""),
            ]);
            assert_eq!(
                fetch(&transport, Some(PATTERN), None, NOW)
                    .await
                    .unwrap_err()
                    .upstream_status,
                Some(404)
            );
            let transport = Fake::new(vec![
                response(503, core::API, b""),
                response(200, "https://evil.example/releases/tag/v5.0.1", b""),
            ]);
            assert_eq!(
                fetch(&transport, Some(PATTERN), None, NOW)
                    .await
                    .unwrap_err()
                    .code,
                "invalid-release-redirect"
            );
            assert_eq!(transport.calls.lock().unwrap().len(), 2);
        });
    }
    #[test]
    fn exact_api_assets_and_download_receipts_are_revalidated() {
        tauri::async_runtime::block_on(async {
            let prior = cache();
            let transport = Fake::new(vec![response(
                200,
                core::API,
                &serde_json::to_vec(&prior.release).unwrap(),
            )]);
            assert_eq!(
                fetch(&transport, Some(PATTERN), None, NOW).await.unwrap().0,
                prior.release
            );
            let mut ambiguous = prior.release.clone();
            ambiguous.assets.push(ambiguous.assets[0].clone());
            let transport = Fake::new(vec![response(
                200,
                core::API,
                &serde_json::to_vec(&ambiguous).unwrap(),
            )]);
            assert_eq!(
                fetch(&transport, Some(PATTERN), None, NOW)
                    .await
                    .unwrap_err()
                    .code,
                "update-asset-mismatch"
            );
            assert_eq!(transport.calls.lock().unwrap().len(), 1);
            let result = core::result(&prior, PATTERN, "5.0.0", "", "github-api", None).unwrap();
            let id = result.candidate_id.clone();
            let selected = Ok(result.clone());
            assert_eq!(
                candidate_url(Some(&selected), &id).unwrap(),
                result.package_url
            );
            assert!(candidate_url(Some(&selected), "renderer URL or old receipt").is_err());
            assert!(candidate_url(None, &id).is_err());
            assert!(candidate_url(
                Some(&Err(Failure::new("network-unavailable", "offline"))),
                &id
            )
            .is_err());
            let mut current = result;
            current.available = false;
            assert!(candidate_url(Some(&Ok(current)), &id).is_err());
        });
    }
    #[test]
    fn an_unconfigured_channel_checks_release_metadata_without_probing_an_asset() {
        tauri::async_runtime::block_on(async {
            let mut prior = cache();
            prior.release.tag_name = "v4.1.0".into();
            prior.release.assets.clear();
            let transport = Fake::new(vec![response(
                200,
                core::API,
                &serde_json::to_vec(&prior.release).unwrap(),
            )]);
            let (release, _, source) = fetch(&transport, None, None, NOW).await.unwrap();
            assert_eq!(release.tag_name, "v4.1.0");
            assert_eq!(source, "github-api");
            assert_eq!(transport.calls.lock().unwrap().len(), 1);
            let checked = Cache {
                schema_version: 1,
                etag: String::new(),
                checked_at: core::time(NOW),
                release,
            };
            assert!(
                !core::result_without_package(&checked, "5.0.0", "", source, None)
                    .unwrap()
                    .available
            );
        });
    }
}
