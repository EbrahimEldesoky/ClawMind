use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use clap::{Parser, Subcommand};
use colored::*;
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use clawmind::{
    create_router, AgentTui, AppState, EngineHandle, HardwareProfile, ToolRegistry,
};

#[derive(Parser, Debug)]
#[command(
    name = "clawmind",
    version,
    about = "Ultra-Fast Hardware-Steered Autonomous AI Agent in Rust"
)]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Path to GGUF model file
    #[arg(short, long, default_value = "models/gemma-4-E2B-it-Q4_K_M.gguf")]
    model: PathBuf,

    /// Host to bind server to
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Port to listen on
    #[arg(short, long, default_value_t = 8080)]
    port: u16,

    /// Run quick performance benchmark and print tokens/sec metrics
    #[arg(long)]
    bench: bool,

    /// Start in HTTP server mode (OpenAI compatible API)
    #[arg(long)]
    server: bool,

    /// Inactivity timeout in seconds before hibernating and releasing model from RAM (0 to disable)
    #[arg(long, default_value_t = 300)]
    idle_timeout: u64,

    /// Start in hibernation mode until the first request arrives
    #[arg(long)]
    lazy: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Launch the interactive autonomous agent terminal (default)
    Agent,
    /// Launch the OpenAI/Ollama-compatible HTTP API server
    Server {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(short, long, default_value_t = 8080)]
        port: u16,
    },
    /// Run hardware inference performance benchmark
    Bench,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = Args::parse();

    // CRITICAL: Set OpenMP to passive wait policy.
    // Without this, llama.cpp's OpenMP threads spin-wait at 100% CPU even when
    // the engine is completely idle (no inference running). This single env var
    // makes threads sleep when not processing, dropping idle CPU from 800% to ~0%.
    unsafe {
        std::env::set_var("OMP_WAIT_POLICY", "PASSIVE");
        if std::env::var("OMP_NUM_THREADS").is_err() {
            let cores = sysinfo::System::new_all().physical_core_count().unwrap_or(4);
            std::env::set_var("OMP_NUM_THREADS", cores.to_string());
        }
    }

    // Determine operation mode
    let is_server_mode = args.server
        || matches!(args.command, Some(Commands::Server { .. }));
    let is_bench_mode = args.bench
        || matches!(args.command, Some(Commands::Bench));

    // Initialize tracing (in agent mode, keep logs quiet so terminal UI is pristine)
    let default_filter = if is_server_mode { "info" } else { "error" };
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| default_filter.into()))
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .init();

    // 1. Hardware Detection & Dynamic Steering
    let hardware = HardwareProfile::detect();
    let mut config = hardware.auto_tune(Some(&args.model));
    config.idle_timeout_secs = args.idle_timeout;

    let model_name = args
        .model
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "gemma-4-model".to_string());

    // Print dashboard in server or bench mode
    if is_server_mode || is_bench_mode {
        hardware.print_dashboard(&config, &model_name);
    }

    // Verify model exists
    if !args.model.exists() {
        eprintln!(
            "{} Model file not found at: {}",
            "❌ ERROR:".bright_red().bold(),
            args.model.display()
        );
        eprintln!("Run ./run.sh to automatically download the Gemma 4 model.");
        std::process::exit(1);
    }

    // 2. Initialize ClawMind Engine
    if is_server_mode || is_bench_mode {
        println!("⏳ {}", "Loading model into dedicated hardware-pinned worker...".cyan());
    }

    let engine = EngineHandle::new(
        args.model.clone(),
        model_name.clone(),
        hardware.clone(),
        config.clone(),
        args.lazy,
    )?;

    // 3. Benchmark Mode
    if is_bench_mode {
        println!("\n🔥 {}", "Running ClawMind Performance Benchmark...".bright_yellow().bold());
        let test_prompt = "<|turn>user\nExplain why Rust is the fastest language for AI inference in 2 sentences.<turn|>\n<|turn>model\n".to_string();

        let _start = Instant::now();
        let mut rx = engine.infer(test_prompt, 128, 0.7, 0.95).await;

        print!("{}", "Generated: ".bright_green());
        while let Some(res) = rx.recv().await {
            match res {
                clawmind::InferResponse::Token(t) => print!("{}", t),
                clawmind::InferResponse::Done { tokens_per_sec, elapsed_secs, completion_tokens, prompt_tokens } => {
                    println!("\n\n📊 {}", "Benchmark Results:".bold().underline());
                    println!("  • Prompt Tokens     : {}", prompt_tokens.to_string().cyan());
                    println!("  • Completion Tokens : {}", completion_tokens.to_string().cyan());
                    println!("  • Total Time        : {:.3} s", elapsed_secs);
                    println!("  • Generation Speed  : {} tokens/sec", format!("{:.2}", tokens_per_sec).bright_green().bold());
                }
                clawmind::InferResponse::Error(e) => {
                    eprintln!("\n{} Benchmark error: {}", "❌".bright_red(), e);
                }
            }
        }
        return Ok(());
    }

    // 4. Server Mode
    if is_server_mode {
        let (host, port) = match &args.command {
            Some(Commands::Server { host, port }) => (host.clone(), *port),
            _ => (args.host.clone(), args.port),
        };

        let bind_addr = format!("{}:{}", host, port);
        let app_state = AppState {
            engine,
            hardware,
        };
        let app = create_router(app_state);

        let socket = socket2::Socket::new(
            socket2::Domain::IPV4,
            socket2::Type::STREAM,
            Some(socket2::Protocol::TCP),
        )?;
        socket.set_reuse_address(true)?;
        socket.set_nonblocking(true)?;
        let addr: std::net::SocketAddr = bind_addr.parse()?;
        socket.bind(&addr.into())?;
        socket.listen(128)?;
        let listener = TcpListener::from_std(std::net::TcpListener::from(socket))?;

        println!("\n{}", "🌐 ClawMind Local AI Server is ONLINE!".bright_green().bold());
        println!("  • OpenAI API Endpoint  : {}", format!("http://{}/v1", bind_addr).bright_cyan().underline());
        println!("  • Chat Completions     : {}", format!("http://{}/v1/chat/completions", bind_addr).bright_cyan());
        println!("  • Ollama Compatibility : {}", format!("http://{}/api/tags", bind_addr).bright_cyan());
        println!("  • Realtime Hardware API: {}", format!("http://{}/system/hardware", bind_addr).bright_cyan());
        println!("{}", "══════════════════════════════════════════════════════════════════".bright_cyan());

        axum::serve(listener, app).await?;
        return Ok(());
    }

    // 5. Default: Autonomous Agent Interactive Terminal (TUI)
    let tool_registry = Arc::new(ToolRegistry::new());
    let tui = AgentTui::new(Arc::new(engine), tool_registry, hardware);

    tui.run_interactive().await?;

    Ok(())
}

