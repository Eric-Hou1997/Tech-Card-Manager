//! Emby owns its web root. TCM owns only explicitly planned assets and its lease.
//! Every multi-file change records verified before/after blobs outside Emby before
//! replacing anything. Recovery refuses third-party changes instead of guessing.
use crate::{hash, paths, AppError, MediaItem, Result, Specs};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};
use ts_rs::TS;

const BEGIN: &str = "<!-- IMDbTechManager WebPatch BEGIN -->";
const END: &str = "<!-- IMDbTechManager WebPatch END -->";
const JS: &str = "technical-specs-card.js";
const DATA: &str = "technical-specs-data.json";
const LANG: &str = "technical-specs-languages.json";
const RUNTIME: &str = "technical-specs-runtime.json";
const ALLOWED: &[&str] = &["index.html", JS, DATA, LANG, RUNTIME, "ownership.json"];

fn io(error: std::io::Error) -> AppError {
    AppError::new(
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            "emby-permission-required"
        } else {
            "emby-io"
        },
        error,
    )
}
fn conflict(path: &Path) -> AppError {
    AppError::new(
        "emby-external-change",
        "Target changed; inspect a new maintenance plan",
    )
    .at(path.display())
}
fn sync_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        File::open(path).and_then(|f| f.sync_all()).map_err(io)?;
    }
    // Windows replacement below uses MOVEFILE_WRITE_THROUGH; directory handles
    // do not support FlushFileBuffers with ordinary user permissions.
    #[cfg(windows)]
    let _ = path;
    Ok(())
}
fn replace(source: &Path, target: &Path, expected: Option<&str>) -> Result<()> {
    if digest(target)?.as_deref() != expected {
        return Err(conflict(target));
    }
    #[cfg(unix)]
    fs::rename(source, target).map_err(io)?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
        }
        let from: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
        // A web server can briefly hold a read handle without delete sharing.
        // Retry only bounded sharing/access errors, with a fresh CAS each time.
        // Do not turn a permanent permission failure into an unbounded worker.
        for attempt in 0..8 {
            if digest(target)?.as_deref() != expected {
                return Err(conflict(target));
            }
            if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 1 | 8) } != 0 {
                break;
            }
            let error = std::io::Error::last_os_error();
            if attempt == 7
                || !matches!(error.raw_os_error(), Some(5 | 32 | 33))
                || fs::metadata(target).is_ok_and(|m| m.permissions().readonly())
            {
                return Err(io(error).at(target.display()));
            }
            std::thread::sleep(std::time::Duration::from_millis(10 << attempt));
        }
    }
    sync_dir(
        target
            .parent()
            .ok_or_else(|| AppError::new("invalid-path", "Missing parent"))?,
    )
}
fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    atomic_inner(path, bytes).map_err(|mut error| {
        if error.path.is_none() {
            error.path = Some(path.to_string_lossy().into());
        }
        error
    })
}
fn atomic_inner(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::new("invalid-path", "Missing parent"))?;
    paths::checked(parent)?;
    if path.exists() {
        paths::checked(path)?;
    }
    let expected = digest(path)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(io)?;
    temp.write_all(bytes).map_err(io)?;
    if path.exists() {
        temp.as_file()
            .set_permissions(fs::metadata(path).map_err(io)?.permissions())
            .map_err(io)?;
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temp.as_file()
                .set_permissions(fs::Permissions::from_mode(0o644))
                .map_err(io)?;
        }
    }
    temp.as_file().sync_all().map_err(io)?;
    replace(temp.path(), path, expected.as_deref())?;
    Ok(())
}
fn bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            paths::checked(path)?;
            if !meta.is_file() || meta.len() > 64 * 1024 * 1024 {
                return Err(AppError::new(
                    "emby-invalid-file",
                    "Expected regular file up to 64 MiB",
                )
                .at(path.display()));
            }
            fs::read(path).map(Some).map_err(io)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(e).at(path.display())),
    }
}
fn digest(path: &Path) -> Result<Option<String>> {
    Ok(bytes(path)?.map(|b| hash(&b)))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicIndex {
    pub version: u32,
    #[serde(rename = "generatedAt")]
    pub generated_at: String,
    pub items: BTreeMap<String, Specs>,
    #[serde(rename = "itemTypes")]
    pub item_types: BTreeMap<String, String>,
}
pub(crate) fn public_imdb(value: &str) -> Option<&str> {
    let value = value.trim();
    let digits = value.strip_prefix("tt")?;
    ((5..=12).contains(&digits.len()) && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .then_some(value)
}
pub fn public_index(items: &[MediaItem], generated_at: String) -> PublicIndex {
    let mut result = PublicIndex {
        version: 7,
        generated_at,
        items: BTreeMap::new(),
        item_types: BTreeMap::new(),
    };
    let mut ordered: Vec<_> = items.iter().collect();
    ordered.sort_by(|a, b| a.path.cmp(&b.path));
    for item in ordered {
        let Some(imdb) = public_imdb(&item.imdb) else {
            continue;
        };
        if item.error.is_some()
            || !matches!(item.kind.as_str(), "Movie" | "Series")
            || item.specs.is_empty()
        {
            continue;
        }
        let entry = result.items.entry(imdb.into()).or_default();
        for (key, values) in &item.specs {
            let merged = entry.entry(key.clone()).or_default();
            for value in values {
                if !merged
                    .iter()
                    .any(|old| old.to_lowercase() == value.to_lowercase())
                {
                    merged.push(value.clone());
                }
            }
        }
        let kind = result
            .item_types
            .entry(imdb.into())
            .or_insert(item.kind.clone());
        if item.kind == "Series" {
            *kind = "Series".into();
        }
    }
    result
}

fn validate_html(input: &[u8]) -> Result<&str> {
    let text = std::str::from_utf8(input).map_err(|e| AppError::new("emby-invalid-index", e))?;
    let lower = text.to_ascii_lowercase(); // ASCII fold preserves UTF-8 byte offsets.
    if input.len() < 256
        || text.contains('\0')
        || ["<html", "<head", "<body", "</body>", "</html>"]
            .iter()
            .any(|s| !lower.contains(s))
    {
        return Err(AppError::new(
            "emby-invalid-index",
            "Emby index structure is incomplete",
        ));
    }
    Ok(text)
}
/// Remove only one exact historical marker block with one known script.
pub fn clean_index(input: &[u8]) -> Result<Vec<u8>> {
    let text = validate_html(input)?;
    let begins: Vec<_> = text.match_indices(BEGIN).collect();
    let ends: Vec<_> = text.match_indices(END).collect();
    let scripts = regex::Regex::new(
        r#"(?i)<script\b[^>]*\bsrc=["']technical-specs-card\.js\?v=[^"']+["'][^>]*>\s*</script>"#,
    )
    .map_err(|e| AppError::new("emby-pattern", e))?;
    if begins.is_empty() && ends.is_empty() && !text.to_ascii_lowercase().contains(JS) {
        return Ok(input.to_vec());
    }
    if begins.len() != 1
        || ends.len() != 1
        || begins[0].0 >= ends[0].0
        || scripts.find_iter(text).count() != 1
    {
        return Err(AppError::new(
            "emby-unknown-ownership",
            "Incomplete or ambiguous historical patch",
        ));
    }
    let start = begins[0].0;
    let end = ends[0].0 + END.len();
    let inside = &text[start + BEGIN.len()..ends[0].0];
    if scripts.find_iter(inside).count() != 1 || !scripts.replace_all(inside, "").trim().is_empty()
    {
        return Err(AppError::new(
            "emby-unknown-ownership",
            "Marker contains unowned content",
        ));
    }
    let mut out = text.to_owned();
    out.replace_range(start..end, "");
    validate_html(out.as_bytes())?;
    Ok(out.into_bytes())
}
fn patched(input: &[u8], javascript: &[u8]) -> Result<Vec<u8>> {
    let clean = clean_index(input)?;
    let text = validate_html(&clean)?;
    let at = text
        .to_ascii_lowercase()
        .rfind("</body>")
        .ok_or_else(|| AppError::new("emby-invalid-index", "No closing body"))?;
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let revision = hash(javascript);
    let patch = format!("{BEGIN}{nl}<script src=\"{JS}?v={revision}\" defer></script>{nl}{END}");
    let mut out = text.to_owned();
    out.insert_str(at, &patch);
    Ok(out.into_bytes())
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Ownership {
    web_root: String,
    assets: BTreeMap<String, String>,
    session: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Change {
    name: String,
    before: Option<String>,
    after: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct MaintenancePlan {
    pub id: String,
    pub action: String,
    pub target: String,
    pub files: Vec<String>,
    pub legacy_patch: bool,
    pub fingerprint: String,
    #[ts(skip)]
    changes: Vec<Change>,
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(skip)]
    request_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    legacy_review: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct IntegrationStatus {
    pub target: String,
    pub installed: bool,
    pub healthy: bool,
    pub phase: String,
    pub issues: Vec<String>,
    pub requires_permission: bool,
    #[serde(default)]
    pub details: Option<IntegrationDetails>,
    #[serde(default)]
    pub legacy_patch: Option<LegacyPatchObservation>,
}

/// A read-only observation of this installation's fixed card files. This does
/// not establish the absence of legacy processes or authorize their cleanup.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct LegacyPatchObservation {
    pub unsafe_patch: bool,
    pub items: Vec<String>,
    pub fingerprint: String,
}

/// Observations of fixed integration files, never evidence of client rendering.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct IntegrationDetails {
    pub script_exists: bool,
    pub script_matches: bool,
    pub script_version: Option<String>,
    pub data_exists: bool,
    pub data_valid: bool,
    pub data_fingerprint: Option<String>,
    pub runtime_valid: bool,
}

pub fn index_fingerprint(index: &PublicIndex) -> Result<String> {
    Ok(hash(&serde_json::to_vec(&(
        &index.items,
        &index.item_types,
    ))?))
}

impl MaintenancePlan {
    fn review_fingerprint(&self) -> Result<String> {
        let base = hash(&serde_json::to_vec(&(
            "tcm-maintenance-review-v2",
            &self.id,
            &self.action,
            &self.target,
            &self.files,
            self.legacy_patch,
            &self.request_fingerprint,
            &self.changes,
        ))?);
        match &self.legacy_review {
            Some(review) => Ok(hash(&serde_json::to_vec(&(
                "tcm-maintenance-review-v3",
                base,
                review,
            ))?)),
            None => Ok(base),
        }
    }
    fn validate_review(&self) -> Result<()> {
        if (self.request_fingerprint.is_some() || self.legacy_review.is_some())
            && self.fingerprint != self.review_fingerprint()?
        {
            return Err(AppError::new(
                "emby-invalid-journal",
                "Reviewed file snapshot does not match its fingerprint",
            ));
        }
        Ok(())
    }
}

pub struct Integration {
    web: PathBuf,
    backup: PathBuf,
    _lock: Option<File>,
    _backup_lock: File,
    // Valid only while this object owns the exclusive private journal lock.
    // Polls recheck unfinished receipts; new mutations and recovery scan history.
    pending: Mutex<Option<BTreeSet<String>>>,
}
impl Integration {
    pub fn open(web: &Path, backup_root: &Path) -> Result<Self> {
        Self::open_mode(web, backup_root, true)
    }
    /// Build a reviewable plan and external backups without writing the Emby tree.
    /// This object can never apply, recover, publish or renew a service lease.
    pub fn inspect_only(web: &Path, backup_root: &Path) -> Result<Self> {
        Self::open_mode(web, backup_root, false)
    }
    pub fn open_accessible(web: &Path, backup_root: &Path) -> Result<Self> {
        match Self::open(web, backup_root) {
            Err(error) if error.code == "emby-permission-required" => {
                Self::inspect_only(web, backup_root)
            }
            result => result,
        }
    }
    fn require_write(&self) -> Result<()> {
        if self._lock.is_none() {
            return Err(AppError::new("emby-permission-required","Review the maintenance plan and authorize access to this Emby installation before writing").at(self.web.display()));
        }
        Ok(())
    }
    fn open_mode(web: &Path, backup_root: &Path, writable: bool) -> Result<Self> {
        let web = paths::checked(web)?;
        validate_html(
            &bytes(&web.join("index.html"))?
                .ok_or_else(|| AppError::new("emby-index-missing", "Select Emby web directory"))?,
        )?;
        fs::create_dir_all(backup_root).map_err(io)?;
        let backup_root = paths::checked(backup_root)?;
        if backup_root.starts_with(&web) {
            return Err(AppError::new(
                "emby-backup-location",
                "Backups must be outside Emby web root",
            ));
        }
        let backup = backup_root.join(hash(web.to_string_lossy().as_bytes()));
        fs::create_dir_all(backup.join("blobs")).map_err(io)?;
        fs::create_dir_all(backup.join("operations")).map_err(io)?;
        paths::checked(&backup)?;
        let backup_lock_path = backup.join(".tcm-backup.lock");
        if backup_lock_path.exists() {
            paths::checked(&backup_lock_path)?;
        }
        let backup_lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&backup_lock_path)
            .map_err(|e| io(e).at(backup_lock_path.display()))?;
        backup_lock
            .try_lock()
            .map_err(|e| AppError::new("emby-busy", e))?;
        // Mutating owners also coordinate across different private backup roots.
        let lock = if writable {
            let lock_path = web.join(".tcm-web.lock");
            if lock_path.exists() {
                paths::checked(&lock_path)?;
            }
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&lock_path)
                .map_err(|e| io(e).at(lock_path.display()))?;
            lock.try_lock().map_err(|e| AppError::new("emby-busy", e))?;
            // A pre-existing writable lock does not prove its directory still
            // permits atomic replacement after an administrator changes access.
            let probe =
                tempfile::NamedTempFile::new_in(&web).map_err(|e| io(e).at(web.display()))?;
            probe.close().map_err(|e| io(e).at(web.display()))?;
            Some(lock)
        } else {
            None
        };
        let integration = Self {
            web,
            backup,
            _lock: lock,
            _backup_lock: backup_lock,
            pending: Mutex::new(None),
        };
        if writable {
            integration.recover()?;
        }
        Ok(integration)
    }
    fn target(&self, name: &str) -> Result<PathBuf> {
        if !ALLOWED.contains(&name) {
            return Err(AppError::new(
                "emby-invalid-journal",
                "Unexpected transaction target",
            ));
        }
        Ok(if name == "ownership.json" {
            self.backup.join(name)
        } else {
            self.web.join(name)
        })
    }
    fn ownership(&self) -> Result<Ownership> {
        let value: Ownership = match bytes(&self.backup.join("ownership.json"))? {
            Some(b) => serde_json::from_slice(&b)?,
            None => Ownership::default(),
        };
        if !value.web_root.is_empty() && value.web_root != self.web.to_string_lossy() {
            return Err(AppError::new(
                "emby-invalid-ownership",
                "Ownership belongs to another web root",
            ));
        }
        Ok(value)
    }
    fn blob(&self, value: &[u8]) -> Result<String> {
        let id = hash(value);
        let path = self.backup.join("blobs").join(&id);
        if !path.exists() {
            atomic(&path, value)?;
        }
        if digest(&path)?.as_deref() != Some(&id) {
            return Err(AppError::new(
                "emby-backup-corrupt",
                "Backup verification failed",
            ));
        }
        Ok(id)
    }
    fn load_blob(&self, id: &str) -> Result<Vec<u8>> {
        if id.len() != 64 || !id.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(AppError::new("emby-invalid-journal", "Invalid blob ID"));
        }
        let value = bytes(&self.backup.join("blobs").join(id))?
            .ok_or_else(|| AppError::new("emby-backup-missing", id))?;
        if hash(&value) != id {
            return Err(AppError::new("emby-backup-corrupt", id));
        }
        Ok(value)
    }
    fn journal_path(&self, id: &str) -> PathBuf {
        self.backup
            .join("operations")
            .join(format!("{}.json", hash(id.as_bytes())))
    }
    fn save_plan(&self, plan: &MaintenancePlan) -> Result<()> {
        let data = serde_json::to_vec(plan)?;
        let mut pending = self.pending.lock().map_err(|_| {
            AppError::new("emby-recovery-required", "Journal tracking was interrupted")
        })?;
        // An uncertain write invalidates the observation, including failures
        // after replacement but before directory sync. Re-read disk next time.
        let previous = pending.take();
        atomic(&self.journal_path(&plan.id), &data)?;
        *pending = previous.map(|mut ids| {
            if plan.phase == "prepared" {
                ids.insert(plan.id.clone());
            } else {
                ids.remove(&plan.id);
            }
            ids
        });
        Ok(())
    }
    fn load_plan(&self, id: &str) -> Result<MaintenancePlan> {
        let data = bytes(&self.journal_path(id))?
            .ok_or_else(|| AppError::new("operation-not-found", id))?;
        let plan: MaintenancePlan = serde_json::from_slice(&data)?;
        if plan.id != id || plan.target != self.web.to_string_lossy() {
            return Err(AppError::new(
                "emby-invalid-journal",
                "Plan target mismatch",
            ));
        }
        plan.validate_review()?;
        Ok(plan)
    }
    /// Read the durable receipt without replaying a mutation after transport loss.
    pub fn operation(&self, id: &str) -> Result<MaintenancePlan> {
        self.load_plan(id)
    }
    pub fn status(&self) -> Result<IntegrationStatus> {
        let owned = self.ownership()?;
        let mut issues = Vec::new();
        for (name, expected) in &owned.assets {
            if digest(&self.target(name)?)?.as_deref() != Some(expected) {
                issues.push(format!("changed:{name}"));
            }
        }
        let index = bytes(&self.web.join("index.html"))?
            .ok_or_else(|| AppError::new("emby-index-missing", "Missing index"))?;
        let text = validate_html(&index)?;
        let installed = text.contains(BEGIN);
        let patch_present =
            installed || text.contains(END) || text.to_ascii_lowercase().contains(JS);
        let malformed_patch = patch_present && clean_index(&index).is_err();
        if malformed_patch {
            issues.push("emby-unknown-ownership".into());
        }
        if patch_present && owned.assets.is_empty() {
            issues.push("legacy-patch-requires-plan".into());
        }
        if !installed && !owned.assets.is_empty() {
            issues.push("emby-upgrade-removed-patch".into());
        }
        if self.pending_recovery()? {
            issues.push("emby-recovery-required".into());
        }
        let healthy = installed && issues.is_empty();
        let script = bytes(&self.web.join(JS))?;
        let data = bytes(&self.web.join(DATA))?;
        let parsed = data
            .as_deref()
            .and_then(|raw| serde_json::from_slice::<PublicIndex>(raw).ok())
            .filter(|index| index.version == 7);
        let runtime = bytes(&self.web.join(RUNTIME))?
            .as_deref()
            .and_then(|raw| serde_json::from_slice::<Lease>(raw).ok());
        let now = chrono::Utc::now();
        let runtime_valid = runtime.is_some_and(|lease| {
            lease.version == 1
                && lease.enabled
                && lease.web_card_version == env!("CARGO_PKG_VERSION")
                && owned.session.as_deref() == Some(&lease.session_id)
                && chrono::DateTime::parse_from_rfc3339(&lease.expires_at).is_ok_and(|expiry| {
                    expiry > now
                        && chrono::DateTime::parse_from_rfc3339(&lease.updated_at).is_ok_and(
                            |updated| {
                                let lifetime =
                                    expiry.signed_duration_since(updated).num_milliseconds();
                                lifetime > 0 && lifetime <= 30_000
                            },
                        )
                })
        });
        let script_matches = script.as_deref().is_some_and(|raw| {
            owned.assets.get(JS) == Some(&hash(raw))
                && raw == include_bytes!("../../../web-card/technical-specs-card.js")
        });
        let script_version = script
            .as_deref()
            .and_then(|raw| std::str::from_utf8(raw).ok())
            .and_then(|text| {
                text.lines().find_map(|line| {
                    let value = line
                        .trim()
                        .strip_prefix("const WEB_CARD_VERSION = ")?
                        .trim_end_matches(';')
                        .trim_matches('"');
                    semver::Version::parse(value).ok().map(|_| value.to_owned())
                })
            });
        let legacy_patch = if malformed_patch || (patch_present && owned.assets.is_empty()) {
            let recognized = script.as_deref().is_some_and(|raw| {
                raw == include_bytes!("../../../web-card/technical-specs-card.js")
                    || hash(raw)
                        == "16ef77094694ddb0bf98f077747992587fa3ab6050c094b45226595b4e564fae"
            });
            let unsafe_patch = malformed_patch || !recognized;
            let reference_version =
                regex::Regex::new(r#"(?i)technical-specs-card\.js\?v=([0-9.]+)["']"#)
                    .map_err(|e| AppError::new("emby-pattern", e))?
                    .captures(text)
                    .map(|captures| captures[1].to_string());
            let item = if unsafe_patch {
                "无法确认所有权的网页补丁（标记、脚本数量或块内容异常）".into()
            } else if let Some(version) = reference_version.or_else(|| script_version.clone()) {
                format!("旧版网页卡片 v{version}")
            } else {
                "检测到旧版组件".into()
            };
            // Bind later confirmation to every file the adoption transaction
            // may replace; an observation never creates a plan or backup blob.
            let mut observed = BTreeMap::new();
            observed.insert("index.html", Some(hash(&index)));
            for name in [JS, DATA, LANG, RUNTIME] {
                observed.insert(name, digest(&self.target(name)?)?);
            }
            Some(LegacyPatchObservation {
                unsafe_patch,
                items: vec![item],
                fingerprint: hash(&serde_json::to_vec(&(&self.web, observed))?),
            })
        } else {
            None
        };
        let details = IntegrationDetails {
            script_exists: script.is_some(),
            script_matches,
            script_version,
            data_exists: data.is_some(),
            data_valid: parsed.is_some(),
            data_fingerprint: parsed.as_ref().map(index_fingerprint).transpose()?,
            runtime_valid,
        };
        Ok(IntegrationStatus {
            target: self.web.to_string_lossy().into(),
            installed,
            healthy,
            phase: if self._lock.is_none() {
                "permission-required"
            } else if healthy {
                "disk-ready"
            } else {
                "unverified"
            }
            .into(),
            requires_permission: self._lock.is_none(),
            details: Some(details),
            legacy_patch,
            issues,
        })
    }
    pub fn publish_index(&self, index: &PublicIndex) -> Result<bool> {
        self.require_index_access()?;
        let mut owner = self.ownership()?;
        let target = self.target(DATA)?;
        let before = bytes(&target)?;
        if before
            .as_ref()
            .is_some_and(|data| owner.assets.get(DATA) != Some(&hash(data)))
        {
            return Err(conflict(&target));
        }
        if let Some(before) = &before {
            let previous: PublicIndex = serde_json::from_slice(before)?;
            if previous.items == index.items && previous.item_types == index.item_types {
                return Ok(false);
            }
        }
        if index.version != 7 {
            return Err(AppError::new(
                "emby-index-schema",
                "Unsupported public data schema",
            ));
        }
        let after = serde_json::to_vec(index)?;
        owner.web_root = self.web.to_string_lossy().into();
        owner.assets.insert(DATA.into(), hash(&after));
        let previous_owner = bytes(&self.backup.join("ownership.json"))?;
        let next_owner = serde_json::to_vec(&owner)?;
        let fingerprint = hash(&serde_json::to_vec(&(
            before.as_ref().map(|data| hash(data)),
            hash(&after),
            previous_owner.as_ref().map(|data| hash(data)),
        ))?);
        let mut plan = MaintenancePlan {
            id: format!("index-{fingerprint}"),
            action: "publish-index".into(),
            target: self.web.to_string_lossy().into(),
            files: vec![DATA.into(), "ownership.json".into()],
            legacy_patch: false,
            legacy_review: None,
            request_fingerprint: Some(fingerprint.clone()),
            fingerprint,
            changes: vec![
                Change {
                    name: DATA.into(),
                    before: before.as_ref().map(|data| self.blob(data)).transpose()?,
                    after: Some(self.blob(&after)?),
                },
                Change {
                    name: "ownership.json".into(),
                    before: previous_owner
                        .as_ref()
                        .map(|data| self.blob(data))
                        .transpose()?,
                    after: Some(self.blob(&next_owner)?),
                },
            ],
            phase: "planned".into(),
        };
        plan.fingerprint = plan.review_fingerprint()?;
        self.save_plan(&plan)?;
        self.apply(&plan.id, &plan.fingerprint)?;
        Ok(true)
    }
    // IndexOnly in the original product never requires a working Web Card.
    // Missing presentation resources may be maintained explicitly later; unknown
    // data/leases, ambiguous patches and unresolved transactions still fail closed.
    pub(crate) fn require_index_access(&self) -> Result<IntegrationStatus> {
        self.require_write()?;
        let status = self.status()?;
        let data = digest(&self.target(DATA)?)?;
        if status.issues.iter().any(|issue| {
            !matches!(issue.as_str(), "emby-upgrade-removed-patch")
                && issue != &format!("changed:{JS}")
                && issue != &format!("changed:{LANG}")
                && !(data.is_none() && issue == &format!("changed:{DATA}"))
        }) {
            return Err(AppError::new(
                "emby-repair-required",
                status.issues.join(", "),
            ));
        }
        let owner = self.ownership()?;
        if data
            .as_ref()
            .is_some_and(|data| owner.assets.get(DATA) != Some(data))
        {
            return Err(conflict(&self.target(DATA)?));
        }
        if let Some(data) = bytes(&self.web.join(RUNTIME))? {
            let lease: Lease = serde_json::from_slice(&data)?;
            if owner.session.as_deref() != Some(&lease.session_id) {
                return Err(AppError::new(
                    "emby-unknown-lease",
                    "Runtime lease belongs to another manager",
                ));
            }
        }
        Ok(status)
    }
    pub fn plan(
        &self,
        id: &str,
        action: &str,
        javascript: &[u8],
        index: &PublicIndex,
        languages: &[u8],
    ) -> Result<MaintenancePlan> {
        self.plan_checked(id, action, javascript, index, languages, None)
    }
    pub fn plan_adoption(
        &self,
        id: &str,
        reviewed: &str,
        javascript: &[u8],
        index: &PublicIndex,
        languages: &[u8],
    ) -> Result<MaintenancePlan> {
        self.plan_checked(id, "adopt", javascript, index, languages, Some(reviewed))
    }
    fn require_legacy_review(&self, reviewed: &str) -> Result<()> {
        if !self
            .status()?
            .legacy_patch
            .is_some_and(|patch| !patch.unsafe_patch && patch.fingerprint == reviewed)
        {
            return Err(AppError::new(
                "emby-legacy-review-changed",
                "旧版网页文件已改变，请重新确认迁移清单",
            )
            .at(self.web.display()));
        }
        Ok(())
    }
    fn plan_checked(
        &self,
        id: &str,
        action: &str,
        javascript: &[u8],
        index: &PublicIndex,
        languages: &[u8],
        reviewed: Option<&str>,
    ) -> Result<MaintenancePlan> {
        self.require_recovered()?;
        if id.is_empty()
            || id.len() > 200
            || !matches!(
                action,
                "install" | "update" | "repair" | "repair-web" | "remove" | "adopt"
            )
        {
            return Err(AppError::new(
                "emby-invalid-operation",
                "Invalid maintenance action or ID",
            ));
        }
        let intent = hash(&serde_json::to_vec(&(
            action,
            hash(javascript),
            index,
            hash(languages),
        ))?);
        let intent = match reviewed {
            Some(review) => hash(&serde_json::to_vec(&(intent, review))?),
            None => intent,
        };
        if self.journal_path(id).exists() {
            let old = self.load_plan(id)?;
            if old.request_fingerprint.as_ref().unwrap_or(&old.fingerprint) != &intent {
                return Err(AppError::new(
                    "operation-conflict",
                    "Operation ID reused with different input",
                ));
            }
            return Ok(old);
        }
        if let Some(reviewed) = reviewed {
            self.require_legacy_review(reviewed)?;
        }
        let current = bytes(&self.web.join("index.html"))?
            .ok_or_else(|| AppError::new("emby-index-missing", "Missing index"))?;
        let clean = clean_index(&current)?;
        let mut owner = self.ownership()?;
        let legacy = owner.assets.is_empty() && current != clean;
        let mut candidates: BTreeMap<String, Option<Vec<u8>>> = BTreeMap::new();
        if action == "adopt" {
            let old_script = digest(&self.web.join(JS))?;
            let recognized = old_script.as_ref().is_some_and(|digest| {
                digest == &hash(javascript)
                    || digest == "16ef77094694ddb0bf98f077747992587fa3ab6050c094b45226595b4e564fae"
            });
            if !legacy || !recognized {
                return Err(AppError::new("emby-legacy-unrecognized", "Legacy migration requires a recognized marker and the exact baseline card script"));
            }
            owner.assets.insert(JS.into(), old_script.unwrap());
            // Explicit legacy migration recognizes the baseline script plus its
            // schema-bound companion data. Every original byte enters the journal.
            if let Some(data) = bytes(&self.web.join(DATA))? {
                let index: PublicIndex = serde_json::from_slice(&data)?;
                if index.version != 7 {
                    return Err(AppError::new(
                        "emby-legacy-schema",
                        "Unsupported old public index schema",
                    ));
                }
                owner.assets.insert(DATA.into(), hash(&data));
            }
            if let Some(data) = bytes(&self.web.join(LANG))? {
                let value: serde_json::Value = serde_json::from_slice(&data)?;
                if value["schema"] != 1 || !value["languages"].is_object() {
                    return Err(AppError::new(
                        "emby-legacy-schema",
                        "Unsupported old card language schema",
                    ));
                }
                owner.assets.insert(LANG.into(), hash(&data));
            }
            if let Some(data) = bytes(&self.web.join(RUNTIME))? {
                let lease: Lease = serde_json::from_slice(&data)?;
                let expiry = chrono::DateTime::parse_from_rfc3339(&lease.expires_at)
                    .map_err(|e| AppError::new("emby-legacy-lease", e))?;
                if lease.version != 1 || (lease.enabled && expiry > chrono::Utc::now()) {
                    return Err(AppError::new("emby-stop-required", "Stop the previous manager and wait for its lease to expire before migration"));
                }
                candidates.insert(RUNTIME.into(), None);
            }
        }
        candidates.insert(
            "index.html".into(),
            Some(if action == "remove" {
                clean
            } else {
                patched(&current, javascript)?
            }),
        );
        if action == "remove" {
            for (name, expected) in &owner.assets {
                if digest(&self.target(name)?)?.as_deref() != Some(expected) {
                    return Err(conflict(&self.target(name)?));
                }
                candidates.insert(name.clone(), None);
            }
            if let Some(data) = bytes(&self.web.join(RUNTIME))? {
                let lease: Lease = serde_json::from_slice(&data)?;
                if owner.session.as_deref() != Some(&lease.session_id) {
                    return Err(AppError::new("emby-unknown-lease", "Runtime is not owned"));
                }
                if lease.enabled {
                    return Err(AppError::new(
                        "emby-stop-required",
                        "Stop the service before removing integration",
                    ));
                }
                candidates.insert(RUNTIME.into(), None);
            }
            owner.session = None;
            owner.assets.clear();
        } else {
            let _: serde_json::Value = serde_json::from_slice(languages)?;
            for (name, data) in [
                (JS, javascript.to_vec()),
                (DATA, serde_json::to_vec(index)?),
                (LANG, languages.to_vec()),
            ] {
                let live = digest(&self.web.join(name))?;
                // Original RepairWebOnly maintains presentation assets, not the
                // NFO index. Preserve the published data byte-for-byte.
                if action == "repair-web" && name == DATA {
                    if let Some(existing) = &live {
                        if owner.assets.get(DATA) != Some(existing) {
                            return Err(AppError::new(
                                "emby-unknown-asset",
                                "Published data is not authoritatively owned",
                            )
                            .at(self.web.join(DATA).display()));
                        }
                        continue;
                    }
                }
                // A historical marker proves only its script placement, not ownership
                // of arbitrary adjacent files. Unknown assets need a separate plan.
                if let Some(existing) = live {
                    if owner.assets.get(name) != Some(&existing) && existing != hash(&data) {
                        return Err(AppError::new(
                            "emby-unknown-asset",
                            "Existing asset is not authoritatively owned",
                        )
                        .at(self.web.join(name).display()));
                    }
                }
                owner.assets.insert(name.into(), hash(&data));
                candidates.insert(name.into(), Some(data));
            }
        }
        owner.web_root = self.web.to_string_lossy().into();
        candidates.insert("ownership.json".into(), Some(serde_json::to_vec(&owner)?));
        let mut changes = Vec::new();
        for (name, value) in candidates {
            let before = bytes(&self.target(&name)?)?
                .map(|v| self.blob(&v))
                .transpose()?;
            let after = value.map(|v| self.blob(&v)).transpose()?;
            if before != after {
                changes.push(Change {
                    name,
                    before,
                    after,
                });
            }
        }
        // Publish resources before injection; remove injection before deleting assets.
        changes.sort_by_key(|c| {
            if c.name == "ownership.json" {
                3
            } else if c.name == "index.html" {
                if action == "remove" {
                    0
                } else {
                    2
                }
            } else {
                1
            }
        });
        let mut plan = MaintenancePlan {
            id: id.into(),
            action: action.into(),
            target: self.web.to_string_lossy().into(),
            files: changes.iter().map(|c| c.name.clone()).collect(),
            legacy_patch: legacy,
            legacy_review: reviewed.map(str::to_owned),
            request_fingerprint: Some(intent.clone()),
            fingerprint: intent,
            changes,
            phase: "planned".into(),
        };
        plan.fingerprint = plan.review_fingerprint()?;
        if let Some(reviewed) = reviewed {
            self.require_legacy_review(reviewed)?;
            for change in &plan.changes {
                if digest(&self.target(&change.name)?)? != change.before {
                    return Err(conflict(&self.target(&change.name)?));
                }
            }
        }
        self.save_plan(&plan)?;
        Ok(plan)
    }
    fn write_change(&self, change: &Change, forward: bool) -> Result<()> {
        self.require_write()?;
        let path = self.target(&change.name)?;
        let (expected, desired) = if forward {
            (&change.before, &change.after)
        } else {
            (&change.after, &change.before)
        };
        let current = digest(&path)?;
        if current == *desired {
            return Ok(());
        }
        if current != *expected {
            return Err(conflict(&path));
        }
        match desired {
            Some(id) => atomic(&path, &self.load_blob(id)?)?,
            None => {
                if path.exists() {
                    fs::remove_file(&path).map_err(io)?;
                    sync_dir(path.parent().unwrap())?;
                }
            }
        }
        if digest(&path)? != *desired {
            return Err(
                AppError::new("emby-postcondition", "File verification failed").at(path.display()),
            );
        }
        Ok(())
    }
    pub fn repair_web(
        &self,
        id: &str,
        javascript: &[u8],
        languages: &[u8],
    ) -> Result<IntegrationStatus> {
        // Only a missing first-run data file receives an empty index. Existing
        // data belongs to the sole publisher and is not rebuilt by this action.
        let plan = self.plan(
            id,
            "repair-web",
            javascript,
            &public_index(&[], String::new()),
            languages,
        )?;
        self.apply(&plan.id, &plan.fingerprint)
    }
    pub fn apply(&self, id: &str, fingerprint: &str) -> Result<IntegrationStatus> {
        self.require_write()?;
        let mut plan = self.load_plan(id)?;
        if plan.fingerprint != fingerprint {
            return Err(AppError::new(
                "emby-plan-mismatch",
                "Reviewed plan does not match",
            ));
        }
        if plan.phase == "committed" {
            return self.status();
        }
        if plan.phase != "planned" {
            return Err(AppError::new(
                "emby-plan-expired",
                "Create a new plan after recovery",
            ));
        }
        // A previously reviewed plan must not overwrite another operation's
        // partial result. Holding write permission does not prove recovery.
        self.require_recovered()?;
        if let Some(reviewed) = &plan.legacy_review {
            self.require_legacy_review(reviewed)?;
        }
        for change in &plan.changes {
            if digest(&self.target(&change.name)?)? != change.before {
                return Err(conflict(&self.target(&change.name)?));
            }
            if let Some(id) = &change.after {
                self.load_blob(id)?;
            }
            if let Some(id) = &change.before {
                self.load_blob(id)?;
            }
        }
        plan.phase = "prepared".into();
        self.save_plan(&plan)?;
        for change in &plan.changes {
            if let Err(error) = self.write_change(change, true) {
                self.recover()?;
                return Err(error);
            }
        }
        plan.phase = "committed".into();
        self.save_plan(&plan)?;
        self.status()
    }
    fn require_recovered(&self) -> Result<()> {
        if !self.refresh_pending()?.is_empty() {
            return Err(AppError::new(
                "emby-recovery-required",
                "An interrupted maintenance operation must be recovered before another operation",
            )
            .at(self.web.display()));
        }
        Ok(())
    }
    fn pending_recovery(&self) -> Result<bool> {
        let mut pending = self.pending.lock().map_err(|_| {
            AppError::new("emby-recovery-required", "Journal tracking was interrupted")
        })?;
        if pending.is_none() {
            *pending = Some(self.scan_pending()?);
        }
        let ids = pending.as_ref().unwrap();
        for id in ids {
            // A deleted, corrupt or externally completed active receipt is not
            // evidence of recovery by this owner. Explicit recovery must verify it.
            if self.load_plan(id)?.phase != "prepared" {
                return Err(AppError::new(
                    "emby-invalid-journal",
                    "An unfinished maintenance receipt changed unexpectedly",
                ));
            }
        }
        Ok(!ids.is_empty())
    }
    fn refresh_pending(&self) -> Result<BTreeSet<String>> {
        let mut pending = self.pending.lock().map_err(|_| {
            AppError::new("emby-recovery-required", "Journal tracking was interrupted")
        })?;
        let ids = self.scan_pending()?;
        if pending
            .as_ref()
            .is_some_and(|previous| !previous.is_subset(&ids))
        {
            return Err(AppError::new(
                "emby-invalid-journal",
                "An unfinished maintenance receipt disappeared or changed unexpectedly",
            ));
        }
        *pending = Some(ids.clone());
        Ok(ids)
    }
    fn scan_pending(&self) -> Result<BTreeSet<String>> {
        let mut ids = BTreeSet::new();
        for file in fs::read_dir(self.backup.join("operations")).map_err(io)? {
            let path = file.map_err(io)?.path();
            if path.extension().and_then(|v| v.to_str()) != Some("json") {
                continue;
            }
            let plan: MaintenancePlan =
                serde_json::from_slice(&bytes(&path)?.ok_or_else(|| {
                    AppError::new("emby-journal-missing", "Journal disappeared")
                })?)?;
            if plan.target != self.web.to_string_lossy() || path != self.journal_path(&plan.id) {
                return Err(AppError::new(
                    "emby-invalid-journal",
                    "Journal identity mismatch",
                ));
            }
            plan.validate_review()?;
            if plan.phase == "prepared" {
                ids.insert(plan.id);
            }
        }
        Ok(ids)
    }
    pub fn recover(&self) -> Result<()> {
        self.require_write()?;
        for id in self.refresh_pending()? {
            let mut plan = self.load_plan(&id)?;
            if plan.phase != "prepared" {
                return Err(AppError::new(
                    "emby-invalid-journal",
                    "An unfinished maintenance receipt changed during recovery",
                ));
            }
            // Validate every recovery precondition before touching any file.
            for c in &plan.changes {
                let current = digest(&self.target(&c.name)?)?;
                if current != c.before && current != c.after {
                    return Err(conflict(&self.target(&c.name)?));
                }
                if let Some(id) = &c.before {
                    self.load_blob(id)?;
                }
            }
            for c in plan.changes.iter().rev() {
                self.write_change(c, false)?;
            }
            plan.phase = "rolled-back".into();
            self.save_plan(&plan)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct Lease {
    pub version: u32,
    pub manager_version: String,
    pub web_card_version: String,
    pub session_id: String,
    pub sequence: u64,
    pub enabled: bool,
    pub updated_at: String,
    pub expires_at: String,
}
pub fn timestamp() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
impl Integration {
    pub fn begin_session(&self, session: &str) -> Result<Lease> {
        self.begin_session_enabled(session, true)
    }
    pub(crate) fn begin_session_enabled(&self, session: &str, enabled: bool) -> Result<Lease> {
        self.require_index_access()?;
        if session.is_empty() {
            return Err(AppError::new("emby-session-invalid", "Missing session ID"));
        }
        let mut owner = self.ownership()?;
        if let Some(data) = bytes(&self.web.join(RUNTIME))? {
            let previous: Lease = serde_json::from_slice(&data)?;
            if owner.session.as_deref() != Some(&previous.session_id) {
                return Err(AppError::new(
                    "emby-unknown-lease",
                    "Runtime lease belongs to another manager",
                ));
            }
            self.renew_session(
                &previous.session_id,
                previous.sequence.saturating_add(1),
                false,
            )?;
        }
        owner.session = Some(session.into());
        owner.web_root = self.web.to_string_lossy().into();
        atomic(
            &self.backup.join("ownership.json"),
            &serde_json::to_vec(&owner)?,
        )?;
        // The previous lease is disabled before ownership changes. The new session
        // can replace it only while this instance owns the shared web-root lock.
        self.write_lease(session, 1, enabled)
    }
    fn write_lease(&self, session: &str, sequence: u64, enabled: bool) -> Result<Lease> {
        let now = chrono::Utc::now();
        let lease = Lease {
            version: 1,
            manager_version: env!("CARGO_PKG_VERSION").into(),
            web_card_version: env!("CARGO_PKG_VERSION").into(),
            session_id: session.into(),
            sequence,
            enabled,
            updated_at: now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            expires_at: (now + chrono::Duration::seconds(if enabled { 8 } else { 0 }))
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        };
        atomic(&self.web.join(RUNTIME), &serde_json::to_vec(&lease)?)?;
        Ok(lease)
    }
    pub fn renew_session(&self, session: &str, sequence: u64, enabled: bool) -> Result<Lease> {
        self.require_write()?;
        if self.ownership()?.session.as_deref() != Some(session) {
            return Err(AppError::new(
                "emby-session-lost",
                "Session no longer owned",
            ));
        }
        if let Some(data) = bytes(&self.web.join(RUNTIME))? {
            let old: Lease = serde_json::from_slice(&data)?;
            if old.session_id != session || sequence <= old.sequence {
                return Err(AppError::new(
                    "emby-session-conflict",
                    "Runtime changed externally or sequence regressed",
                ));
            }
        }
        self.write_lease(session, sequence, enabled)
    }
}

pub fn bundled_card_languages() -> Result<Vec<u8>> {
    let mut languages = serde_json::Map::new();
    for (locale, source) in [
        (
            "fr-FR",
            include_str!("../../../../language-packs/fr-FR/r2/translations.json"),
        ),
        (
            "ru-RU",
            include_str!("../../../../language-packs/ru-RU/r2/translations.json"),
        ),
        (
            "ja-JP",
            include_str!("../../../../language-packs/ja-JP/r2/translations.json"),
        ),
        (
            "es-ES",
            include_str!("../../../../language-packs/es-ES/r2/translations.json"),
        ),
        (
            "th-TH",
            include_str!("../../../../language-packs/th-TH/r2/translations.json"),
        ),
    ] {
        let value: serde_json::Value = serde_json::from_str(source)?;
        let messages = value["web-card"]
            .as_object()
            .ok_or_else(|| AppError::new("language-pack", "Missing card presentation messages"))?;
        let mut output = serde_json::Map::new();
        for (english, translation) in messages {
            let mut hash = 14695981039346656037u64;
            for byte in english.trim().as_bytes() {
                hash ^= *byte as u64;
                hash = hash.wrapping_mul(1099511628211);
            }
            output.insert(format!("legacy.{hash:016x}"), translation.clone());
        }
        languages.insert(locale.into(), serde_json::Value::Object(output));
    }
    Ok(serde_json::to_vec(
        &serde_json::json!({"schema":1,"catalog_app_version":format!("v{}",env!("CARGO_PKG_VERSION")),"languages":languages}),
    )?)
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    fn setup() -> (tempfile::TempDir, Integration) {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let web = base.join("web");
        fs::create_dir(&web).unwrap();
        fs::write(
            web.join("index.html"),
            format!(
                "<html><head></head><body>{}</body></html>",
                "test".repeat(100)
            ),
        )
        .unwrap();
        let integration = Integration::open(&web, &base.join("private")).unwrap();
        (temp, integration)
    }
    #[test]
    fn interrupted_multi_file_install_rolls_back_after_restart() {
        let (_temp, integration) = setup();
        let web = integration.web.clone();
        let private = integration.backup.parent().unwrap().to_path_buf();
        let original = fs::read(web.join("index.html")).unwrap();
        let mut plan = integration
            .plan(
                "interrupt",
                "install",
                b"js",
                &public_index(&[], timestamp()),
                b"{}",
            )
            .unwrap();
        plan.phase = "prepared".into();
        integration.save_plan(&plan).unwrap();
        for change in plan.changes.iter().take(2) {
            integration.write_change(change, true).unwrap();
        }
        drop(integration);
        let restored = Integration::open(&web, &private).unwrap();
        assert_eq!(fs::read(web.join("index.html")).unwrap(), original);
        assert!(!web.join(JS).exists());
        assert!(!web.join(DATA).exists());
        assert_eq!(
            restored.load_plan("interrupt").unwrap().phase,
            "rolled-back"
        );
    }
    #[test]
    fn external_edit_during_interruption_blocks_all_automatic_recovery() {
        let (_temp, integration) = setup();
        let mut plan = integration
            .plan(
                "interrupt",
                "install",
                b"js",
                &public_index(&[], timestamp()),
                b"{}",
            )
            .unwrap();
        plan.phase = "prepared".into();
        integration.save_plan(&plan).unwrap();
        for change in plan.changes.iter().take(2) {
            integration.write_change(change, true).unwrap();
        }
        fs::write(integration.web.join("index.html"), b"external replacement").unwrap();
        let before = digest(&integration.web.join(JS)).unwrap();
        assert!(integration.recover().is_err());
        assert_eq!(digest(&integration.web.join(JS)).unwrap(), before);
        assert_eq!(
            fs::read(integration.web.join("index.html")).unwrap(),
            b"external replacement"
        );
    }
    #[test]
    #[ignore = "Manual status cost observation with 2000 synthetic completed journals"]
    fn status_cost_with_completed_history() {
        let (_temp, integration) = setup();
        let measure = |integration: &Integration| {
            let start = std::time::Instant::now();
            for _ in 0..5 {
                integration.status().unwrap();
            }
            start.elapsed()
        };
        let empty = measure(&integration);
        let index = public_index(&[], timestamp());
        let mut record = integration
            .plan("template", "install", b"js", &index, b"{}")
            .unwrap();
        for n in 0..2000 {
            record.id = format!("completed-{n}");
            record.phase = "committed".into();
            record.fingerprint = record.review_fingerprint().unwrap();
            // Synthetic historical receipts; only the test's private directory.
            fs::write(
                integration.journal_path(&record.id),
                serde_json::to_vec(&record).unwrap(),
            )
            .unwrap();
        }
        // Reopen to include the synthetic receipts in the startup validation.
        let web = integration.web.clone();
        let private = integration.backup.parent().unwrap().to_path_buf();
        drop(integration);
        let start = std::time::Instant::now();
        let integration = Integration::open(&web, &private).unwrap();
        let reopen = start.elapsed();
        let full = measure(&integration);
        println!("five status reads: empty={empty:?}; completed_2000={full:?}; validated_reopen={reopen:?}");
        assert_eq!(
            fs::read_dir(integration.backup.join("operations"))
                .unwrap()
                .count(),
            2001
        );
    }
    #[test]
    fn writable_owner_cannot_prepare_another_operation_during_interrupted_maintenance() {
        let (_temp, integration) = setup();
        let index = public_index(&[], timestamp());
        let mut pending = integration
            .plan("pending", "install", b"js", &index, b"{}")
            .unwrap();
        pending.phase = "prepared".into();
        integration.save_plan(&pending).unwrap();
        let original = fs::read(integration.web.join("index.html")).unwrap();
        let error = integration
            .plan("later", "install", b"other", &index, b"{}")
            .unwrap_err();
        assert_eq!(error.code, "emby-recovery-required");
        assert!(!integration.journal_path("later").exists());
        assert_eq!(
            fs::read(integration.web.join("index.html")).unwrap(),
            original
        );
        assert_eq!(integration.operation("pending").unwrap().phase, "prepared");
        integration.recover().unwrap();
        assert_eq!(
            integration.operation("pending").unwrap().phase,
            "rolled-back"
        );
        let next = integration
            .plan("later", "install", b"other", &index, b"{}")
            .unwrap();
        assert!(
            integration
                .apply(&next.id, &next.fingerprint)
                .unwrap()
                .healthy
        );
    }
    #[test]
    fn missing_active_receipt_blocks_polls_new_plans_and_recovery() {
        let (_temp, integration) = setup();
        let index = public_index(&[], timestamp());
        let mut pending = integration
            .plan("pending", "install", b"js", &index, b"{}")
            .unwrap();
        pending.phase = "prepared".into();
        integration.save_plan(&pending).unwrap();
        let before = fs::read(integration.web.join("index.html")).unwrap();
        fs::remove_file(integration.journal_path(&pending.id)).unwrap();
        // Neither repeated refresh nor explicit recovery may forget the missing
        // receipt merely because it is no longer present in a directory listing.
        for _ in 0..2 {
            assert!(integration.status().is_err());
            assert!(integration.recover().is_err());
            assert!(integration
                .plan("next", "install", b"js", &index, b"{}")
                .is_err());
        }
        assert!(!integration.journal_path("next").exists());
        assert_eq!(
            fs::read(integration.web.join("index.html")).unwrap(),
            before
        );
    }
    #[test]
    fn readonly_reopen_discovers_pending_and_rechecks_its_receipt() {
        let (_temp, integration) = setup();
        let web = integration.web.clone();
        let private = integration.backup.parent().unwrap().to_path_buf();
        let mut pending = integration
            .plan(
                "pending",
                "install",
                b"js",
                &public_index(&[], timestamp()),
                b"{}",
            )
            .unwrap();
        pending.phase = "prepared".into();
        integration.save_plan(&pending).unwrap();
        drop(integration);
        let inspector = Integration::inspect_only(&web, &private).unwrap();
        assert!(inspector
            .status()
            .unwrap()
            .issues
            .contains(&"emby-recovery-required".into()));
        fs::write(inspector.journal_path("pending"), b"broken receipt").unwrap();
        assert!(inspector.status().is_err());
        assert!(!web.join(JS).exists());
    }
    #[test]
    fn completed_history_stays_on_disk_and_is_checked_before_new_mutation_and_reopen() {
        let (_temp, integration) = setup();
        let index = public_index(&[], timestamp());
        let plan = integration
            .plan("install", "install", b"js", &index, b"{}")
            .unwrap();
        integration.apply(&plan.id, &plan.fingerprint).unwrap();
        let path = integration.journal_path(&plan.id);
        let receipt = fs::read(&path).unwrap();
        for _ in 0..5 {
            assert!(integration.status().unwrap().healthy);
        }
        assert_eq!(fs::read(&path).unwrap(), receipt);
        fs::write(&path, b"corrupt historical receipt").unwrap();
        assert!(integration.operation(&plan.id).is_err());
        assert!(integration
            .plan("later", "repair-web", b"changed", &index, b"{}")
            .is_err());
        assert!(!integration.journal_path("later").exists());
        assert_eq!(fs::read(integration.web.join(JS)).unwrap(), b"js");
        assert!(integration.recover().is_err());
        let web = integration.web.clone();
        let private = integration.backup.parent().unwrap().to_path_buf();
        drop(integration);
        assert!(Integration::open(&web, &private).is_err());
    }
    #[test]
    fn failed_journal_write_forces_disk_observation_before_reporting_recovery_state() {
        let (_temp, integration) = setup();
        let index = public_index(&[], timestamp());
        let mut plan = integration
            .plan("pending", "install", b"js", &index, b"{}")
            .unwrap();
        assert!(!integration.pending_recovery().unwrap());
        let path = integration.journal_path(&plan.id);
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        plan.phase = "prepared".into();
        assert!(integration.save_plan(&plan).is_err());
        fs::remove_dir(&path).unwrap();
        // Model the durable prepared receipt left by an uncertain write. The
        // actual failing write above must invalidate the earlier empty result.
        fs::write(&path, serde_json::to_vec(&plan).unwrap()).unwrap();
        assert!(integration
            .status()
            .unwrap()
            .issues
            .contains(&"emby-recovery-required".into()));
        assert_eq!(
            integration
                .plan("next", "install", b"js", &index, b"{}")
                .unwrap_err()
                .code,
            "emby-recovery-required"
        );
        integration.recover().unwrap();
        assert_eq!(
            integration.operation(&plan.id).unwrap().phase,
            "rolled-back"
        );
        assert!(!integration.pending_recovery().unwrap());
    }
    #[test]
    fn previously_prepared_plan_cannot_apply_over_another_unresolved_transaction() {
        let (_temp, integration) = setup();
        let index = public_index(&[], timestamp());
        let later = integration
            .plan("later", "install", b"js", &index, b"{}")
            .unwrap();
        let mut pending = integration
            .plan("pending", "install", b"js", &index, b"{}")
            .unwrap();
        pending.phase = "prepared".into();
        integration.save_plan(&pending).unwrap();
        assert_eq!(
            integration
                .apply(&later.id, &later.fingerprint)
                .unwrap_err()
                .code,
            "emby-recovery-required"
        );
        assert!(!integration.web.join(JS).exists());
        assert_eq!(integration.operation("later").unwrap().phase, "planned");
        integration.recover().unwrap();
        assert!(
            integration
                .apply(&later.id, &later.fingerprint)
                .unwrap()
                .healthy
        );
    }
    #[test]
    fn pending_maintenance_is_unhealthy_and_blocks_start_and_publication_for_a_writer() {
        let (_temp, integration) = setup();
        let index = public_index(&[], timestamp());
        let install = integration
            .plan("install", "install", b"js", &index, b"{}")
            .unwrap();
        integration
            .apply(&install.id, &install.fingerprint)
            .unwrap();
        let mut pending = integration
            .plan("repair", "repair-web", b"new js", &index, b"{}")
            .unwrap();
        pending.phase = "prepared".into();
        integration.save_plan(&pending).unwrap();
        let before = fs::read(integration.web.join(DATA)).unwrap();
        let state = integration.status().unwrap();
        assert!(!state.healthy);
        assert!(state.issues.contains(&"emby-recovery-required".into()));
        assert_eq!(
            integration.begin_session("unexpected").unwrap_err().code,
            "emby-repair-required"
        );
        assert_eq!(
            integration.publish_index(&index).unwrap_err().code,
            "emby-repair-required"
        );
        assert_eq!(fs::read(integration.web.join(DATA)).unwrap(), before);
        integration.recover().unwrap();
        assert!(integration.status().unwrap().healthy);
        let lease = integration.begin_session("after-recovery").unwrap();
        integration
            .renew_session(&lease.session_id, lease.sequence + 1, false)
            .unwrap();
    }
    #[test]
    fn corrupt_backup_prevents_install_before_any_mutation() {
        let (_temp, integration) = setup();
        let plan = integration
            .plan(
                "bad-backup",
                "install",
                b"js",
                &public_index(&[], timestamp()),
                b"{}",
            )
            .unwrap();
        let id = plan
            .changes
            .iter()
            .find(|c| c.before.is_some())
            .unwrap()
            .before
            .as_ref()
            .unwrap();
        fs::write(integration.backup.join("blobs").join(id), b"corruption").unwrap();
        assert!(integration.apply(&plan.id, &plan.fingerprint).is_err());
        assert!(!integration.web.join(JS).exists());
    }
}
