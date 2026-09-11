use product_core::{services::CredentialStore, AppError, Result};
pub struct NativeCredentials {
    service: String,
}
impl NativeCredentials {
    pub fn new(service: String) -> Self {
        Self { service }
    }
    fn entry(&self, account: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.service, account)
            .map_err(|e| AppError::new("credential-store", e))
    }
}
impl CredentialStore for NativeCredentials {
    fn get(&self, account: &str) -> Result<Option<String>> {
        match self.entry(account)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AppError::new("credential-read", e)),
        }
    }
    fn put(&self, account: &str, secret: &str) -> Result<()> {
        self.entry(account)?
            .set_password(secret)
            .map_err(|e| AppError::new("credential-write", e))
    }
    fn delete(&self, account: &str) -> Result<()> {
        match self.entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AppError::new("credential-delete", e)),
        }
    }
}
