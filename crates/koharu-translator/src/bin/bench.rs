use std::{path::PathBuf, time::Instant};

use anyhow::Result;
use clap::Parser;
use koharu_ml::llm::{
    ChatMessage, ChatTemplateOptions, GenerationOptions, Input, Llm, LoadOptions, LoadProfile,
};
use serde::Serialize;

/// Representative page used when `--segments` names a count instead of literal
/// prompts. The strings are benchmark load, not a translation prompt path.
const SYNTHETIC_SEGMENTS: &[&str] = &[
    "おはよう、今日もいい天気だね。",
    "ちょっと待って、まだ準備できてないよ。",
    "あの音、聞こえた？",
    "もう遅い、急ごう。",
    "こんなところで何をしているんだ？",
    "君の名前は？",
    "やめて、離して！",
    "わかった、信じるよ。",
    "ここはどこだ…",
    "助けて！誰か！",
    "It can't be helped.",
    "We have to move now.",
    "Did you hear that too?",
    "This is the last time.",
];

#[derive(Debug, Parser)]
#[command(about = "Benchmark the local llama.cpp translation engine on a representative page")]
struct Args {
    /// Path to a GGUF model file on disk.
    #[arg(long)]
    model: PathBuf,

    /// Quantization label recorded in the report.
    #[arg(long)]
    quantization: Option<String>,

    /// Run on the CPU device.
    #[arg(long)]
    cpu: bool,

    /// Segment count, or literal segment prompts.
    #[arg(long, default_value = "12", num_args = 1..)]
    segments: Vec<String>,

    /// Number of measured page iterations.
    #[arg(long, default_value_t = 1)]
    iterations: usize,

    /// Emit a machine-readable JSON report.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Serialize)]
struct Report {
    model: String,
    quantization: Option<String>,
    cpu: bool,
    segments: usize,
    iterations: usize,
    prompt_tokens: usize,
    generated_tokens: usize,
    prompt_tokens_per_second: f64,
    generated_tokens_per_second: f64,
    page_latency_ms: f64,
    peak_rss_kb: Option<u64>,
    thermal_c: Option<f32>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    anyhow::ensure!(args.iterations > 0, "--iterations must be at least 1");
    let segments = resolve_segments(&args.segments);

    koharu_ml::init().await?;
    let device = koharu_ml::device(args.cpu);
    let options = LoadOptions {
        // Android benchmarks run through the same mobile profile as the app.
        profile: LoadProfile::for_device(),
        ..LoadOptions::default()
    };
    let llm = Llm::load_with_options(device, args.model.clone(), options).await?;
    let prompt = render_prompt(&llm, &segments)?;
    let input = Input::new(&prompt);
    let generation = GenerationOptions::default();

    // Load and warm the model outside the measured region.
    llm.inference(&input, &generation)?;

    let mut prompt_tokens = 0usize;
    let mut generated_tokens = 0usize;
    let mut prompt_duration = std::time::Duration::ZERO;
    let mut generation_duration = std::time::Duration::ZERO;
    let mut wall = std::time::Duration::ZERO;
    for _ in 0..args.iterations {
        let started = Instant::now();
        let output = llm.inference(&input, &generation)?;
        wall += started.elapsed();
        prompt_tokens += output.prompt_tokens;
        generated_tokens += output.generated_tokens;
        prompt_duration += output.prompt_duration;
        generation_duration += output.generation_duration;
    }

    let report = Report {
        model: args.model.display().to_string(),
        quantization: args.quantization,
        cpu: args.cpu,
        segments: segments.len(),
        iterations: args.iterations,
        prompt_tokens,
        generated_tokens,
        prompt_tokens_per_second: rate(prompt_tokens, prompt_duration),
        generated_tokens_per_second: rate(generated_tokens, generation_duration),
        page_latency_ms: wall.as_secs_f64() * 1000.0 / args.iterations as f64,
        peak_rss_kb: peak_rss_kb(),
        thermal_c: thermal_celsius(),
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("model:       {}", report.model);
        if let Some(quantization) = &report.quantization {
            println!("quantization: {quantization}");
        }
        println!(
            "device:      {}",
            if report.cpu { "cpu" } else { "accelerator" }
        );
        println!(
            "segments:    {}, iterations: {}",
            report.segments, report.iterations
        );
        println!(
            "prompt:      {} tokens, {:.2} tok/s",
            report.prompt_tokens, report.prompt_tokens_per_second
        );
        println!(
            "generation:  {} tokens, {:.2} tok/s",
            report.generated_tokens, report.generated_tokens_per_second
        );
        println!("page:        {:.1} ms", report.page_latency_ms);
        match report.peak_rss_kb {
            Some(kb) => println!("peak rss:    {:.1} MB", kb as f64 / 1024.0),
            None => println!("peak rss:    unavailable"),
        }
        match report.thermal_c {
            Some(celsius) => println!("thermal:     {celsius:.1} °C"),
            None => println!("thermal:     unavailable"),
        }
    }
    Ok(())
}

fn resolve_segments(spec: &[String]) -> Vec<String> {
    if let [only] = spec
        && let Ok(count) = only.parse::<usize>()
    {
        return (0..count)
            .map(|index| SYNTHETIC_SEGMENTS[index % SYNTHETIC_SEGMENTS.len()].to_owned())
            .collect();
    }
    spec.to_vec()
}

fn render_prompt(llm: &Llm, segments: &[String]) -> Result<String> {
    let system = "You are a manga translator. Translate every numbered segment, keeping the order and count.";
    let user = segments
        .iter()
        .enumerate()
        .map(|(index, segment)| format!("{}. {segment}", index + 1))
        .collect::<Vec<_>>()
        .join("\n");
    llm.render_chat_prompt_with_options(
        &[ChatMessage::system(system), ChatMessage::user(user)],
        ChatTemplateOptions {
            add_generation_prompt: true,
            enable_thinking: false,
        },
    )
}

fn rate(tokens: usize, duration: std::time::Duration) -> f64 {
    let seconds = duration.as_secs_f64();
    if seconds > 0.0 {
        tokens as f64 / seconds
    } else {
        0.0
    }
}

/// Reads the process high-water mark. `/proc` is absent on some platforms.
fn peak_rss_kb() -> Option<u64> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmHWM:")?
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        })
}

/// Reads the hottest thermal zone, in degrees Celsius. Not every device exposes one.
fn thermal_celsius() -> Option<f32> {
    let entries = std::fs::read_dir("/sys/class/thermal").ok()?;
    entries
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("thermal_zone")
        })
        .filter_map(|entry| std::fs::read_to_string(entry.path().join("temp")).ok())
        .filter_map(|raw| raw.trim().parse::<i32>().ok())
        .filter(|milli| *milli > 0)
        .map(|milli| {
            if milli > 1000 {
                milli as f32 / 1000.0
            } else {
                milli as f32
            }
        })
        .reduce(f32::max)
}
