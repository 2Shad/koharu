use anyhow::Context as _;
#[cfg(not(target_os = "android"))]
use keyring_core::Entry;
use secrecy::{SecretBox, zeroize::Zeroize};
use serde::{Deserialize, Serialize, Serializer};
#[cfg(not(target_os = "android"))]
use std::sync::LazyLock;
use std::sync::{Arc, OnceLock};

pub use secrecy::{ExposeSecret, SerializableSecret};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(transparent)]
pub struct SecretString(SecretBox<SecretValue>);

impl Default for SecretString {
    fn default() -> Self {
        Self::from(String::new())
    }
}

impl From<String> for SecretString {
    fn from(value: String) -> Self {
        Self(SecretBox::new(Box::new(SecretValue(value))))
    }
}

impl From<&str> for SecretString {
    fn from(value: &str) -> Self {
        Self::from(value.to_owned())
    }
}

impl ExposeSecret<str> for SecretString {
    fn expose_secret(&self) -> &str {
        &self.0.expose_secret().0
    }
}

#[derive(Clone, Deserialize)]
#[serde(transparent)]
struct SecretValue(String);

impl Zeroize for SecretValue {
    fn zeroize(&mut self) {
        self.0.zeroize();
    }
}

impl secrecy::CloneableSecret for SecretValue {}

impl Serialize for SecretValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str("[REDACTED]")
    }
}

impl SerializableSecret for SecretValue {}

/// A platform credential store.
///
/// Every platform has exactly one store. The host may replace the platform default
/// with [`install`] before the first secret access; Android relies on that to expose
/// the Keystore through its plugin.
pub trait Backend: Send + Sync {
    fn get(&self, key: &str) -> anyhow::Result<Option<SecretString>>;
    fn set(&self, key: &str, secret: &SecretString) -> anyhow::Result<()>;
    fn delete(&self, key: &str) -> anyhow::Result<()>;
}

static BACKEND: OnceLock<Arc<dyn Backend>> = OnceLock::new();

/// Installs the process-wide credential backend before the first secret access.
///
/// Reinstalling the same backend is a no-op; installing a different one is an error.
pub fn install(backend: Arc<dyn Backend>) -> anyhow::Result<()> {
    if let Some(active) = BACKEND.get() {
        anyhow::ensure!(
            Arc::ptr_eq(active, &backend),
            "a credential backend is already installed"
        );
        return Ok(());
    }

    if let Err(requested) = BACKEND.set(backend) {
        let active = BACKEND
            .get()
            .context("a credential backend was installed concurrently without a value")?;
        anyhow::ensure!(
            Arc::ptr_eq(active, &requested),
            "a credential backend is already installed"
        );
    }
    Ok(())
}

fn backend() -> &'static dyn Backend {
    BACKEND.get_or_init(default_backend).as_ref()
}

#[cfg(not(target_os = "android"))]
fn default_backend() -> Arc<dyn Backend> {
    Arc::new(Keyring)
}

#[cfg(target_os = "android")]
fn default_backend() -> Arc<dyn Backend> {
    Arc::new(Unavailable)
}

#[cfg(not(target_os = "android"))]
const SERVICE: &str = "koharu";

#[cfg(not(target_os = "android"))]
static CREDENTIAL_STORE: LazyLock<Result<(), String>> = LazyLock::new(|| {
    #[cfg(target_os = "linux")]
    {
        linux_keyutils_keyring_store::Store::new()
            .map(|store| keyring_core::set_default_store(store))
            .map_err(|error| error.to_string())
    }
    #[cfg(not(target_os = "linux"))]
    Ok(())
});

/// Desktop credential store backed by the operating system keyring.
#[cfg(not(target_os = "android"))]
struct Keyring;

#[cfg(not(target_os = "android"))]
impl Backend for Keyring {
    fn get(&self, key: &str) -> anyhow::Result<Option<SecretString>> {
        let entry = entry(key)?;
        match entry.get_password() {
            Ok(value) => Ok(Some(SecretString::from(value))),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn set(&self, key: &str, secret: &SecretString) -> anyhow::Result<()> {
        entry(key)?.set_password(secret.expose_secret())?;
        Ok(())
    }

    fn delete(&self, key: &str) -> anyhow::Result<()> {
        match entry(key)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

/// Fail-soft store used until the Android host installs the Keystore backend.
#[cfg(target_os = "android")]
struct Unavailable;

#[cfg(target_os = "android")]
impl Backend for Unavailable {
    fn get(&self, _key: &str) -> anyhow::Result<Option<SecretString>> {
        Ok(None)
    }

    fn set(&self, _key: &str, _secret: &SecretString) -> anyhow::Result<()> {
        anyhow::bail!("Koharu cannot store credentials on Android yet")
    }

    fn delete(&self, _key: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Load a Koharu secret by key, returning `None` when no credential exists.
pub fn get(key: &str) -> anyhow::Result<Option<SecretString>> {
    backend().get(key)
}

/// Store a Koharu secret by key.
pub fn set(key: &str, secret: &SecretString) -> anyhow::Result<()> {
    backend().set(key, secret)
}

/// Delete a Koharu secret by key. Missing credentials are treated as success.
pub fn delete(key: &str) -> anyhow::Result<()> {
    backend().delete(key)
}

#[cfg(not(target_os = "android"))]
fn entry(key: &str) -> anyhow::Result<Entry> {
    match &*CREDENTIAL_STORE {
        Ok(()) => {}
        Err(error) => anyhow::bail!("failed to initialize Linux Keyutils: {error}"),
    }
    #[cfg(target_os = "linux")]
    let entry = Entry::new(SERVICE, key)?;
    #[cfg(not(target_os = "linux"))]
    let entry = keyring::Entry::new(SERVICE, key)?.inner;
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Stub;

    impl Backend for Stub {
        fn get(&self, _key: &str) -> anyhow::Result<Option<SecretString>> {
            Ok(None)
        }

        fn set(&self, _key: &str, _secret: &SecretString) -> anyhow::Result<()> {
            Ok(())
        }

        fn delete(&self, _key: &str) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn install_accepts_one_backend() {
        let backend = Arc::new(Stub);
        install(backend.clone()).unwrap();
        install(backend).unwrap();
        assert!(install(Arc::new(Stub)).is_err());
    }
}
