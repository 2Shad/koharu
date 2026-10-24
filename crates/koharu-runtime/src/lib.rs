mod device;
mod hardware;
mod memory;
mod network;
mod runtime;
mod source;
mod store;

pub mod download;

pub use device::{Backend, Device, DeviceType};
pub use hardware::{Hardware, battery_saver_enabled};
pub use memory::release_freed_memory;
pub use runtime::{Feature, Package, Runtime, Torch};
pub use source::HuggingFaceFile;
pub use store::Store;

/// Builds the shared client used by remote APIs and long-running model requests.
///
/// Connection setup follows the shared HTTP policy. Response reads have no
/// implicit timeout or retry, so callers retain cancellation ownership and
/// non-idempotent generation requests are never replayed automatically.
pub fn http_client() -> anyhow::Result<reqwest::Client> {
    network::http()
}

/// Applies Koharu's TLS trust configuration to a `reqwest` client builder.
///
/// `reqwest`'s `rustls` backend verifies against `rustls-platform-verifier`,
/// which on Android must be initialized through JNI against the host activity
/// before the first handshake and otherwise aborts it. The bundled Mozilla
/// roots stand in there; desktop keeps the platform verifier, which needs no
/// setup and honours the operating system trust store.
pub fn configure_reqwest_tls(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    #[cfg(target_os = "android")]
    {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let tls = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        builder.use_preconfigured_tls(tls)
    }
    #[cfg(not(target_os = "android"))]
    {
        builder
    }
}
