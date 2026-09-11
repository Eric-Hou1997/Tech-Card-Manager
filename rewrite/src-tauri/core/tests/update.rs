use tcm_core::update::*;
fn identity() -> InstallationIdentity {
    InstallationIdentity {
        product: "ITM".into(),
        os: "windows".into(),
        arch: "x86_64".into(),
        channel: "nsis".into(),
    }
}
fn catalog(identity: &InstallationIdentity) -> UpdateCatalog {
    UpdateCatalog {
        schema: 1,
        product: identity.product.clone(),
        version: "4.1.1".into(),
        notes: String::new(),
        platforms: [(
            identity.target(),
            UpdateArtifact {
                product: identity.product.clone(),
                version: "4.1.1".into(),
                target: identity.target(),
                channel: identity.channel.clone(),
                url: "https://example.org/setup.exe".into(),
                sha256: "a".repeat(64),
                signature: "signed".into(),
            },
        )]
        .into(),
    }
}
#[test]
fn emulated_x64_installation_never_switches_to_arm_update() {
    let x64 = identity();
    let mut arm = x64.clone();
    arm.arch = "aarch64".into();
    assert!(select(&catalog(&arm), &x64, "4.1.0").is_err());
    assert!(select(&catalog(&x64), &x64, "4.1.0").unwrap().is_some());
}
#[test]
fn wrong_product_channel_and_artifact_metadata_are_rejected() {
    let id = identity();
    let mut c = catalog(&id);
    c.product = "TCM".into();
    assert!(select(&c, &id, "4.1.0").is_err());
    c.product = "ITM".into();
    c.platforms.get_mut(&id.target()).unwrap().channel = "appimage".into();
    assert!(select(&c, &id, "4.1.0").is_err());
}
#[test]
fn deb_and_rpm_are_owned_by_system_package_manager() {
    for channel in ["deb", "rpm"] {
        let id = InstallationIdentity {
            product: "TCM".into(),
            os: "linux".into(),
            arch: "aarch64".into(),
            channel: channel.into(),
        };
        assert_eq!(
            select(&catalog(&id), &id, "4.1.0").unwrap_err().code,
            "update-package-manager"
        );
    }
}
#[test]
fn downgrade_and_unsigned_or_insecure_artifacts_never_install() {
    let id = identity();
    let mut c = catalog(&id);
    assert!(select(&c, &id, "4.1.2").unwrap().is_none());
    c.platforms.get_mut(&id.target()).unwrap().signature.clear();
    assert!(select(&c, &id, "4.1.0").is_err());
    c.platforms.get_mut(&id.target()).unwrap().signature = "signed".into();
    c.platforms.get_mut(&id.target()).unwrap().url = "http://example.org/setup.exe".into();
    assert!(select(&c, &id, "4.1.0").is_err());
}
#[test]
fn rate_limits_and_unexplained_forbidden_are_distinct() {
    assert_eq!(
        http_failure(403, Some("0"), None).code,
        "update-primary-rate-limit"
    );
    assert_eq!(
        http_failure(403, None, Some("30")).code,
        "update-secondary-throttle"
    );
    assert_eq!(http_failure(403, None, None).code, "update-forbidden");
}
