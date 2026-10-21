#[cfg(not(target_os = "android"))]
use keyring_core::Entry;
use secrecy::{SecretBox, zeroize::Zeroize};
use serde::{Deserialize, Serialize, Serializer};
#[cfg(not(target_os = "android"))]
use std::sync::LazyLock;

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

/// Load a Koharu secret by key, returning `None` when no credential exists.
pub fn get(key: &str) -> anyhow::Result<Option<SecretString>> {
    #[cfg(target_os = "android")]
    {
        // Android has no credential store until the platform plugin provides one.
        let _ = key;
        Ok(None)
    }
    #[cfg(not(target_os = "android"))]
    {
        let entry = entry(key)?;
        match entry.get_password() {
            Ok(value) => Ok(Some(SecretString::from(value))),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
}

/// Store a Koharu secret by key.
pub fn set(key: &str, secret: &SecretString) -> anyhow::Result<()> {
    #[cfg(target_os = "android")]
    {
        let _ = (key, secret);
        anyhow::bail!("Koharu cannot store credentials on Android yet");
    }
    #[cfg(not(target_os = "android"))]
    {
        entry(key)?.set_password(secret.expose_secret())?;
        Ok(())
    }
}

/// Delete a Koharu secret by key. Missing credentials are treated as success.
pub fn delete(key: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "android")]
    {
        // Nothing can have been stored on Android, so there is nothing to delete.
        let _ = key;
        Ok(())
    }
    #[cfg(not(target_os = "android"))]
    {
        match entry(key)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
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
