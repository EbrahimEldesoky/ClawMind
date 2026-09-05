use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Instant;
use colored::*;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc as tokio_mpsc;
use tracing::{error, info, warn};

use crate::hardware::{EngineSteeringConfig, HardwareProfile};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineStatus {
    Hibernating = 0,
    Ready = 1,
    Generating = 2,
}

impl From<u8> for EngineStatus {
    fn from(val: u8) -> Self {
        match val {
            1 => EngineStatus::Ready,
            2 => EngineStatus::Generating,
            _ => EngineStatus::Hibernating,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentPart {
    #[serde(rename = "type", default)]
    pub part_type: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(default)]
    pub content: Option<MessageContent>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<serde_json::Value>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn content_text(&self) -> String {
        match &self.content {
            Some(MessageContent::Text(s)) => s.clone(),
            Some(MessageContent::Parts(parts)) => {
                let mut full = String::new();
                for p in parts {
                    if let Some(ref t) = p.text {
                        full.push_str(t);
                    }
                }
                full
            }
            None => String::new(),
        }
    }
}

pub struct InferJob {
    pub prompt: String,
    pub max_tokens: usize,
    pub temperature: f32,
    pub top_p: f32,
    pub token_sender: tokio_mpsc::UnboundedSender<InferResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub enum InferResponse {
    Token(String),
    Done {
        prompt_tokens: usize,
        completion_tokens: usize,
        elapsed_secs: f64,
        tokens_per_sec: f64,
    },
    Error(String),
}

#[derive(Clone)]
pub struct EngineHandle {
    job_tx: std::sync::mpsc::Sender<InferJob>,
    pub model_name: String,
    pub hardware: HardwareProfile,
    pub config: EngineSteeringConfig,
    state: Arc<AtomicU8>,
}

impl EngineHandle {
    pub fn new(
        model_path: PathBuf,
        model_name: String,
        hardware: HardwareProfile,
        config: EngineSteeringConfig,
        lazy: bool,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let (job_tx, job_rx) = std::sync::mpsc::channel::<InferJob>();
        let state = Arc::new(AtomicU8::new(if lazy { 0 } else { 1 }));
        let worker_state = Arc::clone(&state);

        let worker_model_path = model_path.clone();
        let worker_config = config.clone();
        let idle_timeout_secs = config.idle_timeout_secs;

        std::thread::Builder::new()
            .name("clawmind-inference-worker".to_string())
            .spawn(move || {
                info!("Initializing ClawMind hardware-optimized inference backend...");
                let backend = match LlamaBackend::init() {
                    Ok(b) => b,
                    Err(e) => {
                        error!("Failed to initialize LlamaBackend: {e:?}");
                        return;
                    }
                };

                // If not starting lazy, load model immediately for instant initial availability
                if !lazy {
                    info!(
                        "Pre-loading model '{}' into memory (threads: {} decode / {} prefill)...",
                        worker_model_path.display(),
                        worker_config.threads_decode,
                        worker_config.threads_prefill
                    );

                    let keep_running = activate_and_run(
                        &backend,
                        &worker_model_path,
                        &worker_config,
                        None,
                        &job_rx,
                        idle_timeout_secs,
                        &worker_state,
                    );
                    if !keep_running {
                        return;
                    }
                } else {
                    println!(
                        "{}",
                        "🌙 Starting in Smart Hibernation mode. Model will load automatically on first request.".bright_cyan()
                    );
                    worker_state.store(EngineStatus::Hibernating as u8, Ordering::Release);
                }

                // Hibernation Wakeup Loop:
                // Thread blocks on job_rx.recv() with 0.0% CPU and 0 MB model RAM until an inference job arrives.
                while let Ok(first_job) = job_rx.recv() {
                    println!(
                        "\n{}",
                        "☀️ Incoming request detected! Waking up from hibernation & restoring model...".bright_yellow().bold()
                    );

                    let keep_running = activate_and_run(
                        &backend,
                        &worker_model_path,
                        &worker_config,
                        Some(first_job),
                        &job_rx,
                        idle_timeout_secs,
                        &worker_state,
                    );
                    if !keep_running {
                        break;
                    }
                }
            })?;

        Ok(Self {
            job_tx,
            model_name,
            hardware,
            config,
            state,
        })
    }

    pub fn status(&self) -> EngineStatus {
        EngineStatus::from(self.state.load(Ordering::Acquire))
    }

    pub async fn infer(
        &self,
        prompt: String,
        max_tokens: usize,
        temperature: f32,
        top_p: f32,
    ) -> tokio_mpsc::UnboundedReceiver<InferResponse> {
        let (token_sender, rx) = tokio_mpsc::unbounded_channel();
        let job = InferJob {
            prompt,
            max_tokens,
            temperature,
            top_p,
            token_sender,
        };

        let _ = self.job_tx.send(job);
        rx
    }

    pub fn format_messages_to_prompt(messages: &[ChatMessage]) -> String {
        // Gemma 4 Chat Template Format:
        //   Turn start: <|turn>ROLE\n
        //   Turn end:   <turn|>\n
        //   EOS token ID 106 = <turn|>
        let mut prompt = String::new();
        for msg in messages {
            let text = msg.content_text();
            if text.trim().is_empty() {
                continue;
            }
            match msg.role.to_lowercase().as_str() {
                "system" => {
                    prompt.push_str("<|turn>system\n");
                    prompt.push_str(&text);
                    prompt.push_str("<turn|>\n");
                }
                "user" => {
                    prompt.push_str("<|turn>user\n");
                    prompt.push_str(&text);
                    prompt.push_str("<turn|>\n");
                }
                "assistant" => {
                    prompt.push_str("<|turn>model\n");
                    prompt.push_str(&text);
                    prompt.push_str("<turn|>\n");
                }
                _ => {
                    prompt.push_str("<|turn>user\n");
                    prompt.push_str(&text);
                    prompt.push_str("<turn|>\n");
                }
            }
        }
        // Append model turn indicator to cue generation
        prompt.push_str("<|turn>model\n");
        prompt
    }
}

fn load_model(
    backend: &LlamaBackend,
    path: &Path,
    worker_config: &EngineSteeringConfig,
    mlock: bool,
) -> Result<LlamaModel, String> {
    let mut model_params = LlamaModelParams::default()
        .with_n_gpu_layers(worker_config.n_gpu_layers)
        .with_use_mmap(worker_config.use_mmap)
        .with_use_mlock(mlock);

    match LlamaModel::load_from_file(backend, path, &model_params) {
        Ok(m) => Ok(m),
        Err(e) => {
            warn!("Loading with mlock failed ({e:?}), retrying without mlock...");
            model_params = model_params.with_use_mlock(false);
            LlamaModel::load_from_file(backend, path, &model_params)
                .map_err(|e2| format!("Fatal error loading model from '{}': {e2:?}", path.display()))
        }
    }
}

fn create_ctx<'a>(
    model: &'a LlamaModel,
    backend: &'a LlamaBackend,
    worker_config: &EngineSteeringConfig,
) -> Result<LlamaContext<'a>, String> {
    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(worker_config.n_ctx))
        .with_n_batch(worker_config.n_batch)
        .with_n_ubatch(worker_config.n_ubatch)
        .with_n_threads(worker_config.threads_decode as i32)
        .with_n_threads_batch(worker_config.threads_prefill as i32)
        .with_offload_kqv(worker_config.n_gpu_layers > 0);

    model
        .new_context(backend, ctx_params)
        .map_err(|e| format!("Fatal error creating model context: {e:?}"))
}

fn activate_and_run(
    backend: &LlamaBackend,
    worker_model_path: &Path,
    worker_config: &EngineSteeringConfig,
    first_job: Option<InferJob>,
    job_rx: &std::sync::mpsc::Receiver<InferJob>,
    idle_timeout_secs: u64,
    worker_state: &Arc<AtomicU8>,
) -> bool {
    let model = match load_model(backend, worker_model_path, worker_config, worker_config.use_mlock) {
        Ok(m) => m,
        Err(e) => {
            error!("{e}");
            if let Some(j) = first_job {
                let _ = j.token_sender.send(InferResponse::Error(e));
            }
            worker_state.store(EngineStatus::Hibernating as u8, Ordering::Release);
            return true;
        }
    };

    let ctx = match create_ctx(&model, backend, worker_config) {
        Ok(c) => c,
        Err(e) => {
            error!("{e}");
            if let Some(j) = first_job {
                let _ = j.token_sender.send(InferResponse::Error(e));
            }
            worker_state.store(EngineStatus::Hibernating as u8, Ordering::Release);
            return true;
        }
    };

    println!("{}", "✅ Model activated and ready in RAM!".bright_green().bold());
    worker_state.store(EngineStatus::Ready as u8, Ordering::Release);

    let keep_running = run_active_session(
        &model,
        ctx,
        first_job,
        job_rx,
        worker_config,
        idle_timeout_secs,
        worker_state,
    );

    // Scope ends here: ctx is dropped, model is dropped, RAM freed to OS!
    keep_running
}

fn run_active_session<'a>(
    model: &'a LlamaModel,
    mut ctx: LlamaContext<'a>,
    first_job: Option<InferJob>,
    job_rx: &std::sync::mpsc::Receiver<InferJob>,
    worker_config: &EngineSteeringConfig,
    idle_timeout_secs: u64,
    worker_state: &Arc<AtomicU8>,
) -> bool {
    // Process the pending initial job if waking from hibernation
    if let Some(job) = first_job {
        worker_state.store(EngineStatus::Generating as u8, Ordering::Release);
        run_inference_job(model, &mut ctx, job, worker_config.n_batch);
        worker_state.store(EngineStatus::Ready as u8, Ordering::Release);
    }

    loop {
        if idle_timeout_secs == 0 {
            // Keep-alive indefinitely
            match job_rx.recv() {
                Ok(job) => {
                    worker_state.store(EngineStatus::Generating as u8, Ordering::Release);
                    run_inference_job(model, &mut ctx, job, worker_config.n_batch);
                    worker_state.store(EngineStatus::Ready as u8, Ordering::Release);
                }
                Err(_) => return false,
            }
        } else {
            match job_rx.recv_timeout(std::time::Duration::from_secs(idle_timeout_secs)) {
                Ok(job) => {
                    worker_state.store(EngineStatus::Generating as u8, Ordering::Release);
                    run_inference_job(model, &mut ctx, job, worker_config.n_batch);
                    worker_state.store(EngineStatus::Ready as u8, Ordering::Release);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    info!(
                        "Inactivity detected ({}s without requests). Entering Smart Hibernation mode...",
                        idle_timeout_secs
                    );
                    println!(
                        "\n{}",
                        format!(
                            "🌙 Inactivity detected ({}s idle). Entering Smart Hibernation — RAM and CPU released.",
                            idle_timeout_secs
                        )
                        .bright_black()
                    );
                    worker_state.store(EngineStatus::Hibernating as u8, Ordering::Release);
                    // Returning true exits this active session scope, dropping ctx and model from RAM
                    return true;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return false;
                }
            }
        }
    }
}

fn run_inference_job(
    model: &LlamaModel,
    ctx: &mut LlamaContext,
    job: InferJob,
    n_batch: u32,
) {
    let start_time = Instant::now();

    // 1. Clear KV cache
    ctx.clear_kv_cache();

    // 2. Tokenize prompt
    let prompt_tokens = match model.str_to_token(&job.prompt, AddBos::Always) {
        Ok(tokens) => tokens,
        Err(e) => {
            let _ = job.token_sender.send(InferResponse::Error(format!("Tokenization error: {e:?}")));
            return;
        }
    };

    if prompt_tokens.is_empty() {
        let _ = job.token_sender.send(InferResponse::Error("Empty prompt tokens".to_string()));
        return;
    }

    let n_prompt_tokens = prompt_tokens.len();
    info!("🚀 Processing prompt: {} tokens (batch size: {})...", n_prompt_tokens, n_batch);

    // 3. Batch evaluate prompt tokens
    let mut batch = LlamaBatch::new(n_batch as usize, 1);
    let last_prompt_token_idx = n_prompt_tokens - 1;

    let mut decode_error = false;
    for (i, &token) in prompt_tokens.iter().enumerate() {
        // Fast abort if the client closed connection or timed out
        if job.token_sender.is_closed() {
            warn!("⚡ Client disconnected/timed out during prefill at token {}/{}. Aborting immediately to free CPU!", i, n_prompt_tokens);
            return;
        }

        let is_last = i == last_prompt_token_idx;
        if let Err(e) = batch.add(token, i as i32, &[0], is_last) {
            let _ = job.token_sender.send(InferResponse::Error(format!("Batch add error: {e:?}")));
            decode_error = true;
            break;
        }

        if batch.n_tokens() as u32 >= n_batch || is_last {
            if let Err(e) = ctx.decode(&mut batch) {
                let _ = job.token_sender.send(InferResponse::Error(format!("Context decode error: {e:?}")));
                decode_error = true;
                break;
            }
            if !is_last {
                batch.clear();
            }
        }
    }

    if decode_error {
        return;
    }

    // 4. Initialize dynamic sampler
    let seed = rand_seed();
    let mut sampler = LlamaSampler::chain_simple([
        LlamaSampler::top_k(40),
        LlamaSampler::top_p(job.top_p.clamp(0.01, 1.0), 1),
        LlamaSampler::temp(job.temperature.max(0.01)),
        LlamaSampler::dist(seed),
    ]);

    // 5. Autoregressive token generation loop
    let mut current_pos = n_prompt_tokens as i32;
    let mut completion_tokens = 0;
    let mut decoder = encoding_rs::UTF_8.new_decoder();

    loop {
        if job.token_sender.is_closed() {
            warn!("⚡ Client disconnected during generation. Stopping immediately!");
            break;
        }

        if completion_tokens >= job.max_tokens {
            break;
        }

        let next_token = sampler.sample(ctx, batch.n_tokens() - 1);
        sampler.accept(next_token);
        batch.clear();

        // Check EOS: standard eog, Gemma 4 token 106 (<turn|>), or EOS 1
        if model.is_eog_token(next_token) || next_token.0 == 106 || next_token.0 == 1 {
            break;
        }

        if let Ok(piece) = model.token_to_piece(next_token, &mut decoder, true, None) {
            // Guard against turn markers leaking into output
            if piece.contains("<turn|>") || piece.contains("<|turn>") {
                break;
            }

            if job.token_sender.send(InferResponse::Token(piece)).is_err() {
                // Client disconnected
                break;
            }
        }

        completion_tokens += 1;

        if let Err(e) = batch.add(next_token, current_pos, &[0], true) {
            let _ = job.token_sender.send(InferResponse::Error(format!("Batch add generation error: {e:?}")));
            break;
        }
        current_pos += 1;

        if let Err(e) = ctx.decode(&mut batch) {
            let _ = job.token_sender.send(InferResponse::Error(format!("Decode generation error: {e:?}")));
            break;
        }
    }

    let elapsed = start_time.elapsed().as_secs_f64();
    let tps = if elapsed > 0.0 {
        completion_tokens as f64 / elapsed
    } else {
        0.0
    };

    let _ = job.token_sender.send(InferResponse::Done {
        prompt_tokens: n_prompt_tokens,
        completion_tokens,
        elapsed_secs: elapsed,
        tokens_per_sec: tps,
    });
}

fn rand_seed() -> u32 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    (now.as_nanos() & 0xFFFF_FFFF) as u32
}
