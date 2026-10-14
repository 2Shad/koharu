use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use koharu_config::Config;
use koharu_speech::{Line, Speech, SpeechConfig};

#[derive(Debug, Parser)]
#[command(about = "Voice one dialogue line with Drama 3")]
struct Args {
    /// Performance script, such as "[sighs] Fine. If we miss it, it's on you."
    text: String,

    #[arg(long)]
    character: Option<String>,

    #[arg(long, default_value = "speech.wav")]
    out: PathBuf,

    /// Overrides the configured `audiocpp_server` executable.
    #[arg(long)]
    server: Option<PathBuf>,

    /// Overrides the configured Drama 3 GGUF path.
    #[arg(long)]
    model: Option<PathBuf>,

    #[arg(long)]
    cpu_expert_layers: Option<u32>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,koharu_speech=debug".into()),
        )
        .init();
    let args = Args::parse();

    let mut config = SpeechConfig::load()?.read()?.clone();
    config.server = args.server.or(config.server);
    config.model = args.model.or(config.model);
    config.cpu_expert_layers = args.cpu_expert_layers.or(config.cpu_expert_layers);

    let speech = Speech::from_config(Config::memory(config))?;
    let started = std::time::Instant::now();
    let audio = speech
        .synthesize(Line {
            script: &args.text,
            character: args.character.as_deref(),
        })
        .await?;
    std::fs::write(&args.out, &audio.wav)
        .with_context(|| format!("failed to write {}", args.out.display()))?;
    println!(
        "{}: {:.2}s of {} Hz audio in {:.1}s",
        args.out.display(),
        audio.format.duration.as_secs_f64(),
        audio.format.sample_rate,
        started.elapsed().as_secs_f64(),
    );
    Ok(())
}
