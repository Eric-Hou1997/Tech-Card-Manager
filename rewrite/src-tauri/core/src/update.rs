//! Update identity is the installed application's identity, never the host CPU.
use crate::{AppError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
pub struct InstallationIdentity {
    pub product: String,
    pub os: String,
    pub arch: String,
    pub channel: String,
}
impl InstallationIdentity {
    pub fn target(&self) -> String {
        format!(
            "{}-{}-{}-{}",
            self.product.to_ascii_lowercase(),
            self.os,
            self.arch,
            self.channel
        )
    }
    pub fn package_managed(&self) -> bool {
        matches!(self.channel.as_str(), "deb" | "rpm")
    }
    pub fn validate(&self) -> Result<()> {
        if !matches!(self.product.as_str(), "ITM" | "TCM")
            || !matches!(
                (self.os.as_str(), self.arch.as_str(), self.channel.as_str()),
                ("darwin", "aarch64", "app")
                    | ("windows", "x86_64" | "aarch64", "nsis")
                    | ("linux", "x86_64" | "aarch64", "appimage" | "deb" | "rpm")
            )
        {
            return Err(AppError::new(
                "update-installation-identity",
                "Unsupported product, OS, application architecture or installation channel",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct UpdateArtifact {
    pub product: String,
    pub version: String,
    pub target: String,
    pub channel: String,
    pub url: String,
    pub sha256: String,
    pub signature: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct UpdateCatalog {
    pub schema: u32,
    pub product: String,
    pub version: String,
    pub notes: String,
    pub platforms: BTreeMap<String, UpdateArtifact>,
}
pub fn select<'a>(
    catalog: &'a UpdateCatalog,
    identity: &InstallationIdentity,
    current: &str,
) -> Result<Option<&'a UpdateArtifact>> {
    identity.validate()?;
    if catalog.schema != 1 || catalog.product != identity.product {
        return Err(AppError::new(
            "update-product-mismatch",
            "Catalog belongs to another product or schema",
        ));
    }
    let version = semver::Version::parse(catalog.version.trim_start_matches('v'))
        .map_err(|e| AppError::new("update-version", e))?;
    let current = semver::Version::parse(current.trim_start_matches('v'))
        .map_err(|e| AppError::new("update-version", e))?;
    if !version.pre.is_empty() {
        return Err(AppError::new(
            "update-version",
            "Unreleased update rejected",
        ));
    }
    if version <= current {
        return Ok(None);
    }
    if identity.package_managed() {
        return Err(AppError::new(
            "update-package-manager",
            "Use the configured signed system package repository",
        ));
    }
    let target = identity.target();
    let asset = catalog.platforms.get(&target).ok_or_else(|| {
        AppError::new(
            "update-asset-missing",
            format!("No exact artifact for {target}"),
        )
    })?;
    if asset.product != identity.product
        || asset.version != catalog.version
        || asset.target != target
        || asset.channel != identity.channel
    {
        return Err(AppError::new(
            "update-asset-mismatch",
            "Artifact identity differs from installed product",
        ));
    }
    if asset.sha256.len() != 64
        || !asset.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        || asset.signature.is_empty()
    {
        return Err(AppError::new(
            "update-integrity-metadata",
            "Missing artifact checksum or signature",
        ));
    }
    let url = url::Url::parse(&asset.url).map_err(|e| AppError::new("update-url", e))?;
    if url.scheme() != "https" || url.username() != "" || url.password().is_some() {
        return Err(AppError::new(
            "update-url",
            "Update artifacts require HTTPS without embedded credentials",
        ));
    }
    Ok(Some(asset))
}
pub fn http_failure(status: u16, remaining: Option<&str>, retry_after: Option<&str>) -> AppError {
    let code = match status {
        403 if remaining == Some("0") => "update-primary-rate-limit",
        403 if retry_after.is_some() => "update-secondary-throttle",
        403 => "update-forbidden",
        429 => "update-rate-limit",
        404 => "update-catalog-missing",
        500..=599 => "update-server-unavailable",
        _ => "update-http",
    };
    let mut error = AppError::new(code, format!("Update server returned HTTP {status}"));
    error.retryable =
        matches!(status, 429 | 500..=599) || remaining == Some("0") || retry_after.is_some();
    error
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct UpdateProgress {
    pub operation_id: String,
    pub phase: String,
    pub version: Option<String>,
    pub downloaded: String,
    pub total: Option<String>,
    pub error: Option<AppError>,
}

/// Same Minisign verifier and encoding as the Tauri updater. The underlying
/// Ed25519 public key is unchanged; only its public envelope has been added.
pub fn verify_signature(public_key: &str, signature: &str, bytes: &[u8]) -> Result<()> {
    use base64::Engine;
    let decode = |text: &str| -> Result<String> {
        let raw = base64::engine::general_purpose::STANDARD
            .decode(text.trim())
            .map_err(|_| AppError::new("update-signature-format", "Invalid signature encoding"))?;
        String::from_utf8(raw).map_err(|_| {
            AppError::new("update-signature-format", "Signature envelope is not UTF-8")
        })
    };
    let public = minisign_verify::PublicKey::decode(&decode(public_key)?)
        .map_err(|_| AppError::new("update-trust-key", "Trusted public key is invalid"))?;
    let signature = minisign_verify::Signature::decode(&decode(signature)?)
        .map_err(|_| AppError::new("update-signature-format", "Signature envelope is invalid"))?;
    public.verify(bytes, &signature, true).map_err(|_| {
        AppError::new(
            "update-signature-invalid",
            "Update signature verification failed",
        )
    })
}
