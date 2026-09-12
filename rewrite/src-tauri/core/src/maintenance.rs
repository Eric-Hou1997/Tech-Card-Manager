//! Restricted maintenance protocol for a separately authorized native helper.
//! The caller must authenticate the local transport and choose the target and
//! protected journal before construction. Requests cannot choose paths, scripts,
//! credentials, processes or NFO files. This module itself does not elevate.
use crate::{
    card_service::{CardService, ServiceStatus},
    emby::{self, Integration, IntegrationStatus, MaintenancePlan, PublicIndex},
    AppError, Result,
};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::Path,
    sync::Arc,
};
const MAX_FRAME: usize = 64 * 1024 * 1024;
const SCRIPT: &[u8] = include_bytes!("../../../web-card/technical-specs-card.js");
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Install,
    Update,
    Repair,
    Remove,
    Adopt,
}
impl Action {
    fn name(&self) -> &str {
        match self {
            Self::Install => "install",
            Self::Update => "update",
            Self::Repair => "repair",
            Self::Remove => "remove",
            Self::Adopt => "adopt",
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Command {
    Status {},
    Operation {
        id: String,
    },
    Plan {
        id: String,
        action: Action,
        index: PublicIndex,
    },
    Apply {
        id: String,
        fingerprint: String,
    },
    Publish {
        index: PublicIndex,
    },
    Start {
        session: String,
    },
    Stop {},
    Close {},
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub sequence: u64,
    pub request: Command,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "kebab-case")]
pub enum Outcome {
    Status {
        integration: IntegrationStatus,
        service: ServiceStatus,
    },
    Plan(MaintenancePlan),
    Operation(MaintenancePlan),
    Applied(IntegrationStatus),
    Published(bool),
    Service(ServiceStatus),
    Closed,
    Failed(AppError),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reply {
    pub version: u32,
    pub sequence: u64,
    pub outcome: Outcome,
}
pub struct Session {
    integration: Arc<Integration>,
    service: Option<CardService>,
    last: Option<(u64, String, Reply)>,
    closed: bool,
}
fn invalid(message: &str) -> AppError {
    AppError::new("maintenance-protocol", message)
}
fn validate_index(index: &PublicIndex) -> Result<()> {
    let imdb = |id: &str| {
        id.starts_with("tt")
            && (3..=18).contains(&id.len())
            && id[2..].bytes().all(|c| c.is_ascii_digit())
    };
    if index.version != 7
        || index.items.keys().any(|id| !imdb(id))
        || index.item_types.iter().any(|(id, kind)| {
            !index.items.contains_key(id) || !matches!(kind.as_str(), "Movie" | "Series")
        })
    {
        return Err(invalid("Expected a path-free public schema 7 index"));
    }
    Ok(())
}
impl Session {
    /// Only the authenticated platform launcher supplies these fixed paths.
    /// The journal must be private to the helper's elevated identity in production.
    pub fn open(web: &Path, protected_journal: &Path) -> Result<Self> {
        Ok(Self {
            integration: Arc::new(Integration::open(web, protected_journal)?),
            service: None,
            last: None,
            closed: false,
        })
    }
    fn service_status(&self) -> Result<ServiceStatus> {
        self.service
            .as_ref()
            .map(CardService::status)
            .unwrap_or_else(|| {
                Ok(ServiceStatus {
                    phase: "stopped".into(),
                    lease: None,
                    error: None,
                })
            })
    }
    pub fn stop(&mut self) -> Result<ServiceStatus> {
        let status = match self.service.as_mut() {
            Some(service) => service.stop()?,
            None => self.service_status()?,
        };
        self.service = None;
        Ok(status)
    }
    pub fn dispatch(&mut self, request: Request) -> Result<Reply> {
        if self.closed {
            return Err(invalid("Maintenance session is closed"));
        }
        let fingerprint = crate::hash(&serde_json::to_vec(&request)?);
        if request.version != 1 {
            return Err(invalid("Unsupported maintenance protocol version"));
        }
        if let Some((sequence, previous, reply)) = &self.last {
            if request.sequence == *sequence && fingerprint == *previous {
                return Ok(reply.clone());
            }
            if request.sequence
                != sequence
                    .checked_add(1)
                    .ok_or_else(|| invalid("Sequence exhausted"))?
            {
                return Err(invalid("Out-of-order or conflicting maintenance request"));
            }
        } else if request.sequence != 1 {
            return Err(invalid("First maintenance sequence must be one"));
        }
        let result = self.execute(request.request);
        let reply = Reply {
            version: 1,
            sequence: request.sequence,
            outcome: result.unwrap_or_else(Outcome::Failed),
        };
        self.last = Some((request.sequence, fingerprint, reply.clone()));
        Ok(reply)
    }
    fn execute(&mut self, request: Command) -> Result<Outcome> {
        match request {
            Command::Status {} => Ok(Outcome::Status {
                integration: self.integration.status()?,
                service: self.service_status()?,
            }),
            Command::Operation { id } => Ok(Outcome::Operation(self.integration.operation(&id)?)),
            Command::Plan { id, action, index } => {
                if self.service.is_some() {
                    return Err(AppError::new(
                        "emby-stop-required",
                        "Stop the service before maintenance",
                    ));
                }
                validate_index(&index)?;
                Ok(Outcome::Plan(self.integration.plan(
                    &id,
                    action.name(),
                    SCRIPT,
                    &index,
                    &emby::bundled_card_languages()?,
                )?))
            }
            Command::Apply { id, fingerprint } => {
                if self.service.is_some() {
                    return Err(AppError::new(
                        "emby-stop-required",
                        "Stop the service before maintenance",
                    ));
                }
                Ok(Outcome::Applied(self.integration.apply(&id, &fingerprint)?))
            }
            Command::Publish { index } => {
                validate_index(&index)?;
                Ok(Outcome::Published(self.integration.publish_index(&index)?))
            }
            Command::Start { session } => {
                if self.service.is_none() {
                    self.service = Some(CardService::start(self.integration.clone(), &session)?);
                }
                Ok(Outcome::Service(self.service_status()?))
            }
            Command::Stop {} => Ok(Outcome::Service(self.stop()?)),
            Command::Close {} => {
                self.stop()?;
                self.closed = true;
                Ok(Outcome::Closed)
            }
        }
    }
}
/// Length framing bounds allocation and does not interpret shell command text.
/// The platform transport must supply read/write deadlines and peer verification.
pub fn read_frame<R: Read>(reader: &mut R) -> Result<Option<Vec<u8>>> {
    let mut header = [0; 4];
    let first = loop {
        match reader.read(&mut header[..1]) {
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            result => break result,
        }
    }
    .map_err(|e| AppError::new("maintenance-transport", e))?;
    if first == 0 {
        return Ok(None);
    }
    reader
        .read_exact(&mut header[1..])
        .map_err(|e| AppError::new("maintenance-transport", e))?;
    let size = u32::from_be_bytes(header) as usize;
    if size == 0 || size > MAX_FRAME {
        return Err(invalid("Maintenance frame exceeds its size bound"));
    }
    let mut bytes = vec![0; size];
    reader
        .read_exact(&mut bytes)
        .map_err(|e| AppError::new("maintenance-transport", e))?;
    Ok(Some(bytes))
}
pub fn write_frame<W: Write, T: Serialize>(writer: &mut W, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.is_empty() || bytes.len() > MAX_FRAME {
        return Err(invalid("Maintenance frame exceeds its size bound"));
    }
    writer
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .and_then(|()| writer.write_all(&bytes))
        .and_then(|()| writer.flush())
        .map_err(|e| AppError::new("maintenance-transport", e))
}
pub fn serve<R: Read, W: Write>(
    session: &mut Session,
    reader: &mut R,
    writer: &mut W,
) -> Result<()> {
    let outcome = (|| {
        while let Some(bytes) = read_frame(reader)? {
            let request: Request = serde_json::from_slice(&bytes)
                .map_err(|e| AppError::new("maintenance-protocol", e))?;
            let reply = session.dispatch(request)?;
            write_frame(writer, &reply)?;
            if session.closed {
                break;
            }
        }
        Ok(())
    })();
    // EOF, malformed input, timeout and failed reply delivery all synchronously
    // stop owned workers. Never keep renewing a lease after the parent is lost.
    let stopped = session.stop();
    session.closed = true;
    match (outcome, stopped) {
        (Err(original), Err(cleanup)) => Err(AppError::new(
            "maintenance-cleanup",
            format!("{original}; cleanup: {cleanup}"),
        )),
        (Ok(()), Err(error)) => Err(AppError::new("maintenance-cleanup", error)),
        (result, Ok(_)) => result,
    }
}

/// Owns an already authenticated, deadline-bounded platform channel. Dropping a
/// failed channel makes the helper observe EOF and stop its owned service. An
/// uncertain response is never retried implicitly; reconnect and query Operation.
pub struct Client<T> {
    transport: Option<T>,
    sequence: u64,
}
impl<T: Read + Write> Client<T> {
    pub fn from_authenticated_transport(transport: T) -> Self {
        Self {
            transport: Some(transport),
            sequence: 0,
        }
    }
    pub fn request(&mut self, command: Command) -> Result<Outcome> {
        let mut transport = self.transport.take().ok_or_else(|| {
            invalid("Maintenance channel is closed; reconnect and query the operation receipt")
        })?;
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| invalid("Sequence exhausted"))?;
        let request = Request {
            version: 1,
            sequence,
            request: command,
        };
        let result = (|| {
            write_frame(&mut transport, &request)?;
            let bytes = read_frame(&mut transport)?.ok_or_else(|| {
                AppError::new("maintenance-transport", "Helper closed before its response")
            })?;
            let reply: Reply = serde_json::from_slice(&bytes)
                .map_err(|e| AppError::new("maintenance-protocol", e))?;
            if reply.version != 1 || reply.sequence != sequence {
                return Err(invalid("Helper response does not match this request"));
            }
            let matches = matches!(
                (&request.request, &reply.outcome),
                (_, Outcome::Failed(_))
                    | (Command::Status {}, Outcome::Status { .. })
                    | (Command::Operation { .. }, Outcome::Operation(_))
                    | (Command::Plan { .. }, Outcome::Plan(_))
                    | (Command::Apply { .. }, Outcome::Applied(_))
                    | (Command::Publish { .. }, Outcome::Published(_))
                    | (
                        Command::Start { .. } | Command::Stop {},
                        Outcome::Service(_)
                    )
                    | (Command::Close {}, Outcome::Closed)
            );
            if !matches {
                return Err(invalid("Helper response has the wrong result kind"));
            }
            if let (
                Command::Operation { id } | Command::Plan { id, .. },
                Outcome::Operation(plan) | Outcome::Plan(plan),
            ) = (&request.request, &reply.outcome)
            {
                if &plan.id != id {
                    return Err(invalid("Helper returned another operation's receipt"));
                }
            }
            Ok(reply.outcome)
        })();
        match result {
            Ok(outcome) => {
                self.sequence = sequence;
                // A failed Close retains the channel so cleanup can be inspected.
                if !matches!(outcome, Outcome::Closed) {
                    self.transport = Some(transport);
                }
                Ok(outcome)
            }
            Err(error) => Err(AppError::new(
                "maintenance-result-unverified",
                format!("{error}; the channel was closed and the request was not repeated"),
            )
            .at(match &request.request {
                Command::Operation { id }
                | Command::Plan { id, .. }
                | Command::Apply { id, .. } => id.as_str(),
                _ => "maintenance-session",
            })),
        }
    }
}
