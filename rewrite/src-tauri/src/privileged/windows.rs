use super::{
    worker::{self, Connection, Owner},
    *,
};
use product_core::{
    maintenance::Client,
    windows_authorization::Process,
    windows_transport::{Listener, Pipe},
};
use std::{path::Path, sync::Arc, time::Duration};
const IO: Duration = Duration::from_secs(10);
struct ProcessOwner {
    client: Option<Client<Pipe>>,
    process: Process,
    wait_result: Option<Result<()>>,
}
impl ProcessOwner {
    fn wait(&mut self) -> Result<()> {
        if let Some(result) = &self.wait_result {
            return result.clone();
        }
        let result = match self.process.wait(Duration::from_secs(17)).map_err(|e| AppError::new("maintenance-process", e)) {
            Ok(Some(0)) => Ok(()),
            Ok(Some(code)) => Err(AppError::new("maintenance-helper-failed", format!("Permission helper exited with {code}"))),
            Ok(None) => Err(AppError::new("maintenance-cleanup-unverified", "Helper exit was not confirmed; the closed pipe and idle deadline prevent further lease renewal")),
            Err(error) => Err(error),
        };
        self.wait_result = Some(result.clone());
        result
    }
}
impl Owner for ProcessOwner {
    fn request(&mut self, command: Command) -> Result<Outcome> {
        let client = self.client.as_mut().ok_or_else(|| {
            AppError::new("maintenance-disconnected", "Permission session closed")
        })?;
        if let Some(pipe) = client.transport_mut() {
            pipe.reset(IO);
        }
        match client.request(command)? {
            Outcome::Failed(error) => Err(error),
            outcome => Ok(outcome),
        }
    }
    fn finish(&mut self) -> Result<()> {
        let closing = self.request(Command::Close {}).map(|_| ());
        self.client = None;
        let stopped = self.wait();
        stopped.and(closing)
    }
}
impl Drop for ProcessOwner {
    fn drop(&mut self) {
        self.client = None;
        if let Err(error) = self.wait() {
            eprintln!("maintenance-cleanup: {error}");
        }
    }
}
pub fn launch(web: &Path, store: Arc<product_core::store::Store>) -> Result<Connection> {
    let app = std::env::current_exe().map_err(|e| AppError::new("maintenance-executable", e))?;
    let helper = app
        .parent()
        .ok_or_else(|| AppError::new("maintenance-executable", "Application parent missing"))?
        .join("tcm-maintenance-helper.exe");
    product_core::paths::checked(&helper).map_err(|e| {
        AppError::new(
            "maintenance-helper-missing",
            format!("Reinstall this package; its permission helper is missing or ambiguous: {e}"),
        )
    })?;
    let listener = Listener::create(Duration::from_secs(120))
        .map_err(|e| AppError::new("maintenance-pipe", e))?;
    let arguments = [
        "--serve".into(),
        std::process::id().to_string().into(),
        listener.name().into(),
        web.as_os_str().to_owned(),
    ];
    let process = Process::authorize(&helper, &arguments).map_err(|e| {
        AppError::new(
            match e.raw_os_error() {
                Some(1223) => "maintenance-authorization-cancelled",
                Some(5) => "maintenance-authorization-denied",
                _ => "maintenance-authorization-unavailable",
            },
            e,
        )
    })?;
    let mut owner = ProcessOwner {
        client: None,
        process,
        wait_result: None,
    };
    let pipe = listener
        .accept(
            owner
                .process
                .id()
                .map_err(|e| AppError::new("maintenance-process", e))?,
        )
        .map_err(|e| AppError::new("maintenance-pipe", e))?;
    owner.client = Some(Client::from_authenticated_transport(pipe));
    match owner.request(Command::Status {})? {
        Outcome::Status { .. } => worker::launch(owner, store),
        _ => Err(AppError::new(
            "maintenance-protocol",
            "Invalid authorization reply",
        )),
    }
}
