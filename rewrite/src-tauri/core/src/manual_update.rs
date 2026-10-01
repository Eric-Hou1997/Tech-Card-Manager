//! The v4.1.0 release check and browser-download contract. No installer execution.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Write, path::Path};
use ts_rs::TS;

pub const REPOSITORY: &str = "https://github.com/Eric-Hou1997/Tech-Card-Manager";
pub const API: &str = "https://api.github.com/repos/Eric-Hou1997/Tech-Card-Manager/releases/latest";
pub const TTL: i64 = 6 * 3600;
pub const STALE_LIMIT: i64 = 30 * 86400;
pub type Result<T> = std::result::Result<T, Failure>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
pub struct Failure {
    pub code: String,
    pub message: String,
    pub upstream_status: Option<u16>,
    pub retry_at: Option<String>,
    pub github_request_id: Option<String>,
}
impl Failure {
    pub fn new(code: &str, message: impl ToString) -> Self {
        Self {
            code: code.into(),
            message: message.to_string(),
            upstream_status: None,
            retry_at: None,
            github_request_id: None,
        }
    }
    pub fn retry_time(&self) -> Option<i64> {
        self.retry_at.as_deref().and_then(timestamp)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub published_at: String,
    pub assets: Vec<Asset>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cache {
    pub schema_version: u32,
    pub etag: String,
    pub checked_at: String,
    pub release: Release,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct ReleaseCheck {
    pub current_version: String,
    pub latest_version: String,
    pub available: bool,
    pub release_url: String,
    pub package_name: String,
    pub package_url: String,
    pub published_at: String,
    pub portable_directory: String,
    pub source: String,
    pub checked_at: String,
    pub warning: Option<String>,
    pub warning_code: Option<String>,
    pub retry_at: Option<String>,
    pub candidate_id: String,
}

pub fn stable_version(tag: &str) -> Result<semver::Version> {
    let raw = tag.strip_prefix('v').unwrap_or(tag);
    if raw.split('.').count() != 3 || raw.chars().any(|c| !c.is_ascii_digit() && c != '.') {
        return Err(Failure::new(
            "invalid-release",
            "GitHub 最新发布不是可用的正式 vX.Y.Z 版本",
        ));
    }
    semver::Version::parse(raw).map_err(|_| {
        Failure::new(
            "invalid-release",
            "GitHub 最新发布不是可用的正式 vX.Y.Z 版本",
        )
    })
}

/// Patterns are release inputs, never guessed from the host CPU or an asset suffix.
/// The released Windows Portable baseline uses TCM-v{version}-Windows-x64-EXE.zip.
pub fn package_name(pattern: &str, tag: &str) -> Result<String> {
    let version = stable_version(tag)?;
    if pattern.matches("{version}").count() != 1 {
        return Err(Failure::new(
            "update-package-not-configured",
            "此安装渠道尚未配置正式安装包名称",
        ));
    }
    let name = pattern.replace("{version}", &version.to_string());
    if name.len() > 240
        || !name.starts_with("TCM-")
        || name
            .chars()
            .any(|c| !c.is_ascii_alphanumeric() && !"._-".contains(c))
    {
        return Err(Failure::new(
            "update-package-not-configured",
            "此安装渠道的正式安装包名称无效",
        ));
    }
    Ok(name)
}
pub fn installation_pattern<'a>(
    identity: &crate::update::InstallationIdentity,
    configured_target: Option<&str>,
    configured_pattern: Option<&'a str>,
) -> Result<&'a str> {
    identity
        .validate()
        .map_err(|e| Failure::new(&e.code, e.message))?;
    if configured_target != Some(identity.target().as_str()) {
        return Err(Failure::new(
            "update-package-not-configured",
            "此安装渠道尚未配置正式安装包名称",
        ));
    }
    let pattern = configured_pattern.unwrap_or("");
    package_name(pattern, env!("CARGO_PKG_VERSION"))?;
    Ok(pattern)
}
pub fn asset_url(tag: &str, name: &str) -> String {
    format!("{REPOSITORY}/releases/download/{tag}/{name}")
}
/// The same nine-package source is consumed by the bundler and manual updater.
/// Linux selects its installed channel, rather than a single build-time suffix.
pub fn release_pattern(identity: &crate::update::InstallationIdentity) -> Result<&'static str> {
    static PACKAGES: std::sync::LazyLock<serde_json::Value> = std::sync::LazyLock::new(|| {
        serde_json::from_str(include_str!("../assets/release-packages.json"))
            .expect("validated release package catalog")
    });
    let target = identity.target();
    installation_pattern(
        identity,
        Some(&target),
        PACKAGES["packages"][&target]["pattern"].as_str(),
    )
}
pub fn official_redirect(raw: &str) -> bool {
    url::Url::parse(raw).is_ok_and(|url| {
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none()
            && url.host_str().is_some_and(|h| {
                h == "github.com" || h == "api.github.com" || h.ends_with(".githubusercontent.com")
            })
    })
}
pub fn release_tag(raw: &str) -> Result<String> {
    let tag = raw
        .strip_prefix(&format!("{REPOSITORY}/releases/tag/"))
        .ok_or_else(|| {
            Failure::new(
                "invalid-release-redirect",
                "GitHub 最新发布页面没有返回有效的正式版本",
            )
        })?;
    stable_version(tag)?;
    Ok(tag.into())
}
pub fn validate_release(release: &Release) -> Result<semver::Version> {
    if release.draft || release.prerelease {
        return Err(Failure::new(
            "invalid-release",
            "GitHub 最新发布不是可用的正式 vX.Y.Z 版本",
        ));
    }
    stable_version(&release.tag_name)
}
pub fn select<'a>(release: &'a Release, pattern: &str) -> Result<&'a Asset> {
    validate_release(release)?;
    let name = package_name(pattern, &release.tag_name)?;
    let mut matches = release.assets.iter().filter(|a| a.name == name);
    let asset = matches.next().ok_or_else(|| {
        Failure::new(
            "update-asset-missing",
            format!("该正式发布缺少指定更新包 {name}"),
        )
    })?;
    if matches.next().is_some() || asset.browser_download_url != asset_url(&release.tag_name, &name)
    {
        return Err(Failure::new(
            "update-asset-mismatch",
            "正式发布返回了重复、非官方或不匹配的更新地址",
        ));
    }
    Ok(asset)
}
pub fn timestamp(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|d| d.timestamp())
}
pub fn time(value: i64) -> String {
    DateTime::<Utc>::from_timestamp(value, 0)
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default()
}
pub fn load_cache(path: &Path, pattern: &str, now: i64) -> Option<Cache> {
    let cache = load_metadata_cache(path, now)?;
    select(&cache.release, pattern).ok()?;
    Some(cache)
}
pub fn load_metadata_cache(path: &Path, now: i64) -> Option<Cache> {
    let metadata = path.symlink_metadata().ok()?;
    if !metadata.is_file() || metadata.len() > 2 * 1024 * 1024 {
        return None;
    }
    let cache: Cache = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    (cache.schema_version == 1
        && timestamp(&cache.checked_at).is_some_and(|t| t > 0 && t <= now + 300)
        && validate_release(&cache.release).is_ok())
    .then_some(cache)
}
pub fn save_cache(path: &Path, cache: &Cache) -> Result<()> {
    let save = || -> std::io::Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| std::io::Error::other("cache parent missing"))?;
        std::fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(&serde_json::to_vec_pretty(cache)?)?;
        file.as_file().sync_all()?;
        file.persist(path).map_err(|e| e.error)?;
        #[cfg(unix)]
        std::fs::File::open(parent)?.sync_all()?;
        Ok(())
    };
    save().map_err(|e| {
        Failure::new(
            "update-cache-write-failed",
            format!("已检查到更新，但无法保存更新状态：{e}"),
        )
    })
}
pub fn result(
    cache: &Cache,
    pattern: &str,
    current: &str,
    directory: &str,
    source: &str,
    warning: Option<&Failure>,
) -> Result<ReleaseCheck> {
    let asset = select(&cache.release, pattern)?;
    let available = stable_version(&cache.release.tag_name)? > stable_version(current)?;
    // Include the checked time so a later check invalidates an earlier confirmation.
    let candidate_id = crate::hash(
        format!(
            "{}\n{}\n{}",
            asset.browser_download_url, cache.checked_at, pattern
        )
        .as_bytes(),
    );
    let release_url = format!("{REPOSITORY}/releases/tag/{}", cache.release.tag_name);
    Ok(ReleaseCheck {
        current_version: format!("v{}", stable_version(current)?),
        latest_version: cache.release.tag_name.clone(),
        available,
        release_url,
        package_name: asset.name.clone(),
        package_url: asset.browser_download_url.clone(),
        published_at: cache.release.published_at.clone(),
        portable_directory: directory.into(),
        source: source.into(),
        checked_at: cache.checked_at.clone(),
        warning: warning.map(|w| w.message.clone()),
        warning_code: warning.map(|w| w.code.clone()),
        retry_at: warning.and_then(|w| w.retry_at.clone()),
        candidate_id,
    })
}

/// A source checkout can still report that it is current before formal package
/// names are bound. A newer release remains blocked until its exact target asset
/// is configured and validated.
pub fn result_without_package(
    cache: &Cache,
    current: &str,
    directory: &str,
    source: &str,
    warning: Option<&Failure>,
) -> Result<ReleaseCheck> {
    let latest = validate_release(&cache.release)?;
    let current_version = stable_version(current)?;
    if latest > current_version {
        return Err(Failure::new(
            "update-package-not-configured",
            "此安装渠道尚未配置正式安装包名称",
        ));
    }
    Ok(ReleaseCheck {
        current_version: format!("v{current_version}"),
        latest_version: cache.release.tag_name.clone(),
        available: false,
        release_url: format!("{REPOSITORY}/releases/tag/{}", cache.release.tag_name),
        package_name: String::new(),
        package_url: String::new(),
        published_at: cache.release.published_at.clone(),
        portable_directory: directory.into(),
        source: source.into(),
        checked_at: cache.checked_at.clone(),
        warning: warning.map(|item| item.message.clone()),
        warning_code: warning.map(|item| item.code.clone()),
        retry_at: warning.and_then(|item| item.retry_at.clone()),
        candidate_id: String::new(),
    })
}

#[derive(Default)]
pub struct State {
    pub cache: Option<Cache>,
    pub cooldown: Option<Failure>,
}
impl State {
    pub fn cached(
        &self,
        force: bool,
        now: i64,
    ) -> Option<Result<(&Cache, &'static str, Option<&Failure>)>> {
        if !force {
            if let Some(cache) = &self.cache {
                if timestamp(&cache.checked_at).is_some_and(|t| now - t < TTL) {
                    return Some(Ok((cache, "cache", None)));
                }
            }
        }
        if let Some(failure) = &self.cooldown {
            if failure.retry_time().is_some_and(|t| now < t) {
                return Some(self.stale(failure.clone(), now));
            }
        }
        None
    }
    fn stale(
        &self,
        failure: Failure,
        now: i64,
    ) -> Result<(&Cache, &'static str, Option<&Failure>)> {
        if let Some(cache) = &self.cache {
            if timestamp(&cache.checked_at).is_some_and(|t| now - t <= STALE_LIMIT) {
                return Ok((cache, "stale-cache", self.cooldown.as_ref()));
            }
        }
        Err(failure)
    }
    pub fn failed(
        &mut self,
        failure: Failure,
        now: i64,
    ) -> Result<(&Cache, &'static str, Option<&Failure>)> {
        self.cooldown = Some(failure.clone());
        self.stale(failure, now)
    }
}

pub fn http_failure(
    status: u16,
    headers: &BTreeMap<String, String>,
    body: &[u8],
    now: i64,
) -> Failure {
    let header = |name: &str| headers.get(name).map(|v| v.trim()).unwrap_or("");
    let lower = String::from_utf8_lossy(body).to_lowercase();
    let retry = header("x-ratelimit-reset")
        .parse::<i64>()
        .ok()
        .filter(|v| *v > 0)
        .or_else(|| {
            header("retry-after")
                .parse::<i64>()
                .ok()
                .filter(|v| *v >= 0)
                .and_then(|v| now.checked_add(v))
                .or_else(|| {
                    DateTime::parse_from_rfc2822(header("retry-after"))
                        .ok()
                        .map(|v| v.timestamp())
                })
        });
    let (code, message) = if matches!(status, 403 | 429) && header("x-ratelimit-remaining") == "0" {
        (
            "github-primary-rate-limit",
            "GitHub 匿名 API 的出口 IP 额度已用完".into(),
        )
    } else if matches!(status, 403 | 429)
        && (!header("retry-after").is_empty()
            || lower.contains("secondary rate limit")
            || lower.contains("abuse detection"))
    {
        (
            "github-secondary-rate-limit",
            "GitHub 触发了次级限流，请按提示时间后再检查".into(),
        )
    } else if matches!(status, 403 | 429)
        && header("x-github-request-id").is_empty()
        && !lower.contains("github")
    {
        ("proxy-forbidden", "代理或中间网络拒绝了更新请求".into())
    } else if matches!(status, 403 | 429) {
        (
            "github-forbidden",
            "GitHub 拒绝了更新请求，但未标明为额度耗尽".into(),
        )
    } else if status >= 500 {
        ("github-unavailable", "GitHub 更新服务暂时不可用".into())
    } else {
        (
            "github-http-error",
            format!("GitHub 更新请求失败（HTTP {status}）"),
        )
    };
    let mut failure = Failure::new(code, message);
    failure.upstream_status = Some(status);
    failure.github_request_id =
        (!header("x-github-request-id").is_empty()).then(|| header("x-github-request-id").into());
    failure.retry_at = retry
        .or_else(|| {
            matches!(
                code,
                "github-primary-rate-limit" | "github-secondary-rate-limit"
            )
            .then_some(now + 60)
        })
        .map(time);
    failure
}
