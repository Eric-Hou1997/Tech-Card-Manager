use std::collections::BTreeMap;
use tcm_core::manual_update::*;
const PATTERN: &str = "TCM-v{version}-Windows-x64-EXE.zip";
const NOW: i64 = 1_789_257_600;
#[test]
fn release_inputs_are_bound_to_the_installed_architecture_and_channel() {
    let identity = tcm_core::update::InstallationIdentity {
        product: "TCM".into(),
        os: "windows".into(),
        arch: "x86_64".into(),
        channel: "nsis".into(),
    };
    let pattern = "TCM-v{version}-fixture-setup.exe";
    assert_eq!(
        installation_pattern(&identity, Some("tcm-windows-x86_64-nsis"), Some(pattern)).unwrap(),
        pattern
    );
    for target in [
        None,
        Some("tcm-windows-aarch64-nsis"),
        Some("tcm-windows-x86_64-portable"),
        Some("itm-windows-x86_64-nsis"),
    ] {
        assert!(installation_pattern(&identity, target, Some(pattern)).is_err());
    }
    assert!(installation_pattern(&identity, Some("tcm-windows-x86_64-nsis"), None).is_err());
}
fn release() -> Release {
    let tag = "v5.0.1";
    let name = package_name(PATTERN, tag).unwrap();
    Release {
        tag_name: tag.into(),
        html_url: "https://untrusted.example".into(),
        draft: false,
        prerelease: false,
        published_at: String::new(),
        assets: vec![Asset {
            browser_download_url: asset_url(tag, &name),
            name,
        }],
    }
}
fn cache(age: i64) -> Cache {
    Cache {
        schema_version: 1,
        etag: "etag".into(),
        checked_at: time(NOW - age),
        release: release(),
    }
}
#[test]
fn selection_matches_the_released_portable_name_and_rejects_ambiguous_or_foreign_assets() {
    assert_eq!(
        package_name(PATTERN, "5.0.1").unwrap(),
        "TCM-v5.0.1-Windows-x64-EXE.zip"
    );
    for tag in [
        "v5.0.1-beta",
        "v5.0.1+build",
        "v5.0",
        "v5.0.1/next",
        " v5.0.1",
    ] {
        assert!(stable_version(tag).is_err());
    }
    let original = release();
    assert!(select(&original, PATTERN).is_ok());
    let mut other = original.clone();
    other.assets.push(other.assets[0].clone());
    assert_eq!(
        select(&other, PATTERN).unwrap_err().code,
        "update-asset-mismatch"
    );
    for url in [
        "https://evil.example/setup.exe",
        &format!("{}?download=1", original.assets[0].browser_download_url),
        &format!("{}#fragment", original.assets[0].browser_download_url),
    ] {
        let mut other = original.clone();
        other.assets[0].browser_download_url = url.into();
        assert!(select(&other, PATTERN).is_err());
    }
    let mut other = original.clone();
    other.assets.clear();
    assert_eq!(
        select(&other, PATTERN).unwrap_err().code,
        "update-asset-missing"
    );
    for flag in [false, true] {
        let mut other = original.clone();
        other.draft = flag;
        other.prerelease = !flag;
        assert!(select(&other, PATTERN).is_err());
    }
    for pattern in [
        "",
        "../TCM-{version}.zip",
        "ITM-{version}.zip",
        "TCM-{version}-{version}.zip",
    ] {
        assert!(package_name(pattern, "v4.1.1").is_err());
    }
}
#[test]
fn release_and_redirect_urls_cannot_change_product_scheme_or_host() {
    assert_eq!(
        release_tag(&format!("{REPOSITORY}/releases/tag/v4.1.1")).unwrap(),
        "v4.1.1"
    );
    for url in [
        "http://github.com/",
        "https://github.com.evil.example/",
        "https://user@github.com/",
        "https://github.com:444/",
        "https://evilgithubusercontent.com/",
    ] {
        assert!(!official_redirect(url));
    }
    assert!(official_redirect(
        "https://release-assets.githubusercontent.com/asset"
    ));
    assert!(release_tag(&format!("{REPOSITORY}/releases/tag/v4.1.1?x=1")).is_err());
}
#[test]
fn cache_is_atomic_private_and_revalidated_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("update-state.json");
    let cached = cache(20);
    save_cache(&path, &cached).unwrap();
    let loaded = load_cache(&path, PATTERN, NOW).unwrap();
    assert_eq!(loaded.release, cached.release);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert!(load_cache(&path, "TCM-v{version}-other.zip", NOW).is_none());
    let mut corrupt = cached.clone();
    corrupt.checked_at = time(NOW + 301);
    save_cache(&path, &corrupt).unwrap();
    assert!(load_cache(&path, PATTERN, NOW).is_none());
    corrupt = cached;
    corrupt.schema_version = 2;
    save_cache(&path, &corrupt).unwrap();
    assert!(load_cache(&path, PATTERN, NOW).is_none());
}
#[test]
fn an_unconfigured_channel_can_report_current_but_cannot_offer_a_new_release() {
    let current = cache(20);
    let result =
        result_without_package(&current, "5.0.1", "/Applications", "github-api", None).unwrap();
    assert!(!result.available);
    assert_eq!(result.current_version, "v5.0.1");
    assert!(result.package_name.is_empty());
    assert!(result.package_url.is_empty());
    assert!(result.candidate_id.is_empty());
    assert_eq!(
        result_without_package(&current, "5.0.0", "/Applications", "github-api", None)
            .unwrap_err()
            .code,
        "update-package-not-configured"
    );
}
#[test]
fn fresh_stale_expired_force_and_cooldown_preserve_the_original_decisions() {
    let mut state = State {
        cache: Some(cache(10)),
        cooldown: None,
    };
    assert_eq!(state.cached(false, NOW).unwrap().unwrap().1, "cache");
    assert!(state.cached(true, NOW).is_none());
    let mut failure = Failure::new("github-primary-rate-limit", "limited");
    failure.retry_at = Some(time(NOW + 60));
    assert_eq!(state.failed(failure.clone(), NOW).unwrap().1, "stale-cache");
    assert_eq!(
        state.cached(true, NOW).unwrap().unwrap().2.unwrap().code,
        failure.code
    );
    assert!(state.cached(true, NOW + 61).is_none());
    state.cache = Some(cache(TTL));
    assert!(state.cached(false, NOW + 61).is_none());
    state.cache = Some(cache(STALE_LIMIT + 1));
    assert!(state.failed(failure.clone(), NOW).is_err());
    assert!(state.cached(true, NOW).unwrap().is_err());
    state.cache = Some(cache(STALE_LIMIT));
    assert!(state.failed(failure, NOW).is_ok());
}
#[test]
fn http_failures_keep_distinct_reasons_and_retry_deadlines() {
    let headers = |values: &[(&str, &str)]| -> BTreeMap<String, String> {
        values
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    };
    let primary = http_failure(
        403,
        &headers(&[
            ("x-ratelimit-remaining", "0"),
            ("x-ratelimit-reset", "1789257900"),
            ("x-github-request-id", "id"),
        ]),
        b"",
        NOW,
    );
    assert_eq!(primary.code, "github-primary-rate-limit");
    assert_eq!(primary.retry_time(), Some(1789257900));
    assert_eq!(primary.github_request_id.as_deref(), Some("id"));
    let secondary = http_failure(
        429,
        &headers(&[("retry-after", "120")]),
        b"secondary rate limit",
        NOW,
    );
    assert_eq!(secondary.code, "github-secondary-rate-limit");
    assert_eq!(secondary.retry_time(), Some(NOW + 120));
    assert_eq!(
        http_failure(403, &headers(&[]), b"proxy denied", NOW).code,
        "proxy-forbidden"
    );
    assert_eq!(
        http_failure(
            403,
            &headers(&[("x-github-request-id", "id")]),
            b"denied",
            NOW
        )
        .code,
        "github-forbidden"
    );
    assert_eq!(
        http_failure(503, &headers(&[]), b"", NOW).code,
        "github-unavailable"
    );
    assert_eq!(
        http_failure(404, &headers(&[]), b"", NOW).code,
        "github-http-error"
    );
}
#[test]
fn checked_result_never_uses_an_untrusted_release_page_and_does_not_downgrade() {
    let cached = cache(0);
    let check = result(&cached, PATTERN, "5.0.0", "C:\\TCM", "github-api", None).unwrap();
    assert!(check.available);
    assert_eq!(
        check.release_url,
        format!("{REPOSITORY}/releases/tag/v5.0.1")
    );
    assert!(
        !result(&cached, PATTERN, "5.0.1", "", "cache", None)
            .unwrap()
            .available
    );
    assert!(
        !result(&cached, PATTERN, "5.1.0", "", "cache", None)
            .unwrap()
            .available
    );
    let newer = cache(-1);
    assert_ne!(
        check.candidate_id,
        result(&newer, PATTERN, "5.0.0", "", "cache", None)
            .unwrap()
            .candidate_id
    );
}
#[test]
fn release_downloads_match_all_nine_installation_channels() {
    let catalog: serde_json::Value =
        serde_json::from_str(include_str!("../assets/release-packages.json")).unwrap();
    let packages = catalog["packages"].as_object().unwrap();
    assert_eq!(packages.len(), 9);
    let mut names = std::collections::BTreeSet::new();
    for (target, descriptor) in packages {
        let parts: Vec<_> = target.split('-').collect();
        let identity = tcm_core::update::InstallationIdentity {
            product: "TCM".into(),
            os: parts[1].into(),
            arch: parts[2].into(),
            channel: parts[3].into(),
        };
        let pattern = release_pattern(&identity).unwrap();
        assert_eq!(pattern, descriptor["pattern"].as_str().unwrap());
        assert!(names.insert(package_name(pattern, "v5.0.0").unwrap()));
    }
    let wrong_product = tcm_core::update::InstallationIdentity {
        product: "ITM".into(),
        os: "darwin".into(),
        arch: "aarch64".into(),
        channel: "app".into(),
    };
    assert!(release_pattern(&wrong_product).is_err());
}
