//! `audiocpp_server` child serving Drama 3 on a private loopback port; killed
//! when [`Server`] drops.

use std::{
    collections::VecDeque,
    net::{Ipv4Addr, TcpListener},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use serde_json::json;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::{Child, Command},
    time::{Instant, sleep},
};

use crate::{SpeechConfig, model};

#[cfg(windows)]
mod job;
#[cfg(target_os = "linux")]
mod parent;

pub(crate) const MODEL_ID: &str = "drama-3";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);
// Expert-offloaded lines are slow; queued requests must not time out.
const BUSY_TIMEOUT_MS: u64 = 15 * 60 * 1000;
// Frees VRAM for pipeline models between playback bursts.
const IDLE_UNLOAD_MS: u64 = 2 * 60 * 1000;
// Output lines kept for crash reports.
const OUTPUT_TAIL: usize = 12;

/// Settings whose change restarts the server.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ServerSettings {
    server: PathBuf,
    library_paths: Vec<PathBuf>,
    model: PathBuf,
    cpu_expert_layers: u32,
    context_length: u32,
    threads: u32,
}

impl ServerSettings {
    /// Downloads the managed server on first use unless `config.server` is
    /// set; never downloads the model.
    pub(crate) async fn resolve(config: &SpeechConfig) -> Result<Self> {
        // The server resolves relative paths against its own directory.
        let model = std::path::absolute(model::path(config)?)?;
        let (server, library_paths) = match &config.server {
            Some(server) if server.is_file() => (server.clone(), Vec::new()),
            Some(server) => bail!(
                "speech server executable {} does not exist",
                server.display()
            ),
            None => {
                let runtime = koharu_runtime::speech_server().await?;
                (runtime.path, runtime.library_paths)
            }
        };
        Ok(Self {
            server,
            library_paths,
            model,
            cpu_expert_layers: config.cpu_expert_layers.unwrap_or_else(|| {
                cpu_expert_layers(
                    koharu_runtime::Hardware::discover()
                        .device()
                        .map(|device| device.memory_total),
                )
            }),
            context_length: config.context_length,
            threads: config.threads.max(1),
        })
    }
}

/// Q2_K routed-expert layers kept in host RAM: upstream uses 8 on 16 GB; an
/// RTX 3060 (12 GB) needs 16 (10 OOMs); 24 GB holds all.
fn cpu_expert_layers(vram_bytes: Option<usize>) -> u32 {
    const GIB: f64 = (1u64 << 30) as f64;
    match vram_bytes.map(|bytes| bytes as f64 / GIB) {
        Some(gib) if gib >= 22.0 => 0,
        Some(gib) if gib >= 15.0 => 8,
        Some(gib) if gib >= 11.0 => 16,
        _ => 24,
    }
}

pub(crate) struct Server {
    settings: ServerSettings,
    endpoint: String,
    #[cfg(windows)]
    _job: job::KillOnClose,
    child: Child,
    output: Arc<Mutex<VecDeque<String>>>,
    _directory: tempfile::TempDir,
}

impl Server {
    pub(crate) async fn start(settings: ServerSettings, client: &reqwest::Client) -> Result<Self> {
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .context("failed to reserve a loopback port for the speech server")?
            .local_addr()?
            .port();
        let directory = tempfile::Builder::new()
            .prefix("koharu-speech-")
            .tempdir()
            .context("failed to create the speech server directory")?;
        let config = directory.path().join("server.json");
        std::fs::write(&config, server_config(&settings, port).to_string())
            .context("failed to write the speech server config")?;

        let mut command = Command::new(&settings.server);
        command
            .arg("--config")
            .arg(&config)
            .current_dir(settings.server.parent().unwrap_or(Path::new(".")))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if !settings.library_paths.is_empty() {
            // CUDA libraries live in Koharu's store, off the system search path.
            let variable = if cfg!(windows) {
                "PATH"
            } else {
                "LD_LIBRARY_PATH"
            };
            let inherited = std::env::var_os(variable).unwrap_or_default();
            let search = std::env::join_paths(
                settings
                    .library_paths
                    .iter()
                    .cloned()
                    .chain(std::env::split_paths(&inherited)),
            )
            .context("speech runtime library path is not valid")?;
            command.env(variable, search);
        }
        #[cfg(windows)]
        command.creation_flags(windows::Win32::System::Threading::CREATE_NO_WINDOW.0);
        #[cfg(target_os = "linux")]
        parent::kill_with_parent(&mut command);
        let mut child = command
            .spawn()
            .with_context(|| format!("failed to start {}", settings.server.display()))?;
        #[cfg(windows)]
        let job = job::KillOnClose::assign(&child)?;
        let output = Arc::new(Mutex::new(VecDeque::with_capacity(OUTPUT_TAIL)));
        forward(child.stdout.take(), output.clone());
        forward(child.stderr.take(), output.clone());

        let endpoint = format!("http://127.0.0.1:{port}");
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            if let Some(status) = child.try_wait()? {
                bail!(
                    "speech server exited during startup with {status}{}",
                    tail(&output)
                );
            }
            let health = client
                .get(format!("{endpoint}/health"))
                .timeout(Duration::from_secs(2))
                .send()
                .await;
            if health.is_ok_and(|response| response.status().is_success()) {
                break;
            }
            if Instant::now() >= deadline {
                bail!("speech server did not become ready within {STARTUP_TIMEOUT:?}");
            }
            sleep(Duration::from_millis(250)).await;
        }
        tracing::info!(%endpoint, model = %settings.model.display(), "speech server started");
        Ok(Self {
            settings,
            endpoint,
            #[cfg(windows)]
            _job: job,
            child,
            output,
            _directory: directory,
        })
    }

    pub(crate) fn settings(&self) -> &ServerSettings {
        &self.settings
    }

    pub(crate) fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub(crate) fn exited(&mut self) -> bool {
        !matches!(self.child.try_wait(), Ok(None))
    }

    /// Exit status and output tail once the process has exited.
    pub(crate) fn exit_report(&mut self) -> Option<String> {
        let status = self.child.try_wait().ok().flatten()?;
        Some(format!("exited with {status}{}", tail(&self.output)))
    }
}

fn tail(output: &Mutex<VecDeque<String>>) -> String {
    match output.lock() {
        Ok(lines) if !lines.is_empty() => {
            let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
            format!("; last output:\n{}", lines.join("\n"))
        }
        _ => String::new(),
    }
}

fn server_config(settings: &ServerSettings, port: u16) -> serde_json::Value {
    json!({
        "host": "127.0.0.1",
        "port": port,
        "backend": "cuda",
        "threads": settings.threads,
        "lazy_load": true,
        "max_loaded_models": 1,
        "idle_unload_ms": IDLE_UNLOAD_MS,
        "busy_timeout_ms": BUSY_TIMEOUT_MS,
        "ui": false,
        "models": [{
            "id": MODEL_ID,
            "family": "fish_drama3",
            "path": settings.model,
            "task": "tts",
            "mode": "streaming",
            "session_options": {
                "fish_drama3.cpu_expert_layers": settings.cpu_expert_layers,
                "fish_drama3.context_length": settings.context_length,
            },
        }],
    })
}

/// Drains a pipe until EOF so the server never blocks on a full buffer.
fn forward(
    stream: Option<impl AsyncRead + Unpin + Send + 'static>,
    output: Arc<Mutex<VecDeque<String>>>,
) {
    let Some(stream) = stream else {
        return;
    };
    tokio::spawn(async move {
        let mut reader = BufReader::new(stream);
        let mut buffer = Vec::new();
        loop {
            buffer.clear();
            match reader.read_until(b'\n', &mut buffer).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let line = String::from_utf8_lossy(&buffer).trim_end().to_owned();
            tracing::debug!(target: "koharu_speech::server", "{line}");
            // Per-frame CUDA graph notices would evict the failure lines.
            if line.contains("CUDA Graph") || line.contains("graph warmup") {
                continue;
            }
            if let Ok(mut output) = output.lock() {
                if output.len() == OUTPUT_TAIL {
                    output.pop_front();
                }
                output.push_back(line);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_config_matches_the_drama3_contract() {
        let settings = ServerSettings {
            server: "audiocpp_server.exe".into(),
            library_paths: Vec::new(),
            model: "drama.gguf".into(),
            cpu_expert_layers: 16,
            context_length: 2048,
            threads: 8,
        };
        let config = server_config(&settings, 8035);
        assert_eq!(config["port"], 8035);
        assert_eq!(config["models"][0]["family"], "fish_drama3");
        assert_eq!(
            config["models"][0]["session_options"]["fish_drama3.cpu_expert_layers"],
            16
        );
    }

    #[test]
    fn expert_offload_follows_vram() {
        const GIB: usize = 1 << 30;
        assert_eq!(cpu_expert_layers(Some(24 * GIB)), 0);
        assert_eq!(cpu_expert_layers(Some(16 * GIB)), 8);
        // RTX 3060 reports just under 12 GiB.
        assert_eq!(cpu_expert_layers(Some(12_287 * (1 << 20))), 16);
        assert_eq!(cpu_expert_layers(Some(8 * GIB)), 24);
        assert_eq!(cpu_expert_layers(None), 24);
    }
}
