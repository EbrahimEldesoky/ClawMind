# ClawMind System Design and Architecture

## 1. Architectural Overview

ClawMind is structured as a decoupled, multi-threaded autonomous AI agent and inference engine designed for high hardware efficiency, predictable latency, and full operating system control. The system consists of five primary subsystems:

1. Autonomous Agent and ReAct Loop (`agent/`): Coordinates user goals, maintains interaction context, parses model action intents, and orchestrates multi-step tool executions through an interactive terminal interface (TUI).
2. Native Tool Execution Layer (`tools/`): Executes system commands, file operations, Google Chrome browser automation, and mouse/keyboard GUI interactions directly in Rust with safety timeouts and directory tracking.
3. Universal Hardware Orchestrator (`hardware.rs`): Identifies CPU topology, platform instruction sets, physical and virtual memory capacity, and discrete/unified GPU accelerators. Calculates execution parameters dynamically.
4. Inference Engine Runtime (`engine.rs`): Encapsulates model lifecycle, KV-cache allocation, tensor batching, and sampling within an isolated, hardware-pinned worker thread with smart hibernation.
5. Asynchronous HTTP Gateway (`server.rs`): Axum-based non-blocking server providing optional OpenAI-compliant endpoints (`/v1/chat/completions`, `/v1/models`) and Ollama compatibility layers.

```mermaid
graph TD
    subgraph User Interface Layer
        A[Interactive Agent TUI]
        B[External HTTP API Clients]
    end

    subgraph Autonomous Agent Subsystem
        C[Agent Loop ReAct Engine]
        D[Token-Efficient Prompt Builder]
        E[Action Parser: Colon / JSON / CodeBlock]
    end

    subgraph Native Tool Execution Layer
        F[Shell Tool: bash / zsh]
        G[Filesystem Tool: read / write / delete / list / analyze]
        H[Browser Tool: Chrome Search / Open / Fetch]
        I[Mouse Tool: Move / Click / Type / Keys]
    end

    subgraph Inference Engine Runtime (Isolated Thread)
        J[EngineHandle / Job Queue mpsc]
        K[Gemma 4 Context Manager]
        L[Smart Inactivity Hibernator]
        M[Autoregressive Decode Pipeline]
    end

    subgraph Operating System & Hardware Layer
        N[Host Filesystem & Terminal]
        O[Google Chrome Browser Instance]
        P[X11 / Wayland / macOS Display Server]
        Q[CPU Physical Cores & Vector Units]
    end

    A -->|User Goal / Prompt| C
    B -->|HTTP Request| J
    C -->|Constructed Turn Prompt| J
    J --> K
    K --> M
    M -->|Streamed Tokens| C
    C --> E
    E -->|Action Dispatched| F
    E -->|Action Dispatched| G
    E -->|Action Dispatched| H
    E -->|Action Dispatched| I
    F --> N
    G --> N
    H --> O
    I --> P
    K -.-> Q
    L -.->|Auto RAM Unload on Idle| K
```

## 2. Autonomous Agent Architecture

### 2.1 ReAct Loop Implementation

The agent subsystem executes a multi-step Reason-Act (ReAct) loop:

1. Goal Ingestion: The user inputs an objective via the interactive terminal interface.
2. Context Construction: The system prompt and the rolling interaction history are formatted using the Gemma chat template (`<|turn>system`, `<|turn>user`, `<|turn>model`).
3. Model Inference: Tokens stream in real time from the inference worker.
4. Intent Detection: The parser scans generated tokens for structured actions:
   - Colon format: `Action: <tool_name>: <args>`
   - JSON format: `{"action": "<tool_name>", ...}`
   - Code block format: `Action: bash\n```bash\n<cmd>\n```
   - Completion format: `Action: done: <final response>`
5. Tool Execution: The tool registry dispatches the request to the appropriate native tool module.
6. Observation Feedback: The observation output is fed back into the conversation context as a new turn.
7. Iteration: The cycle repeats until the agent declares completion or reaches the maximum step threshold (default: 8 steps).

### 2.2 Token-Budget Prompt Optimization

CPU inference speed is inversely proportional to prompt context length during prefill. While decoding generates tokens at 12-18 tokens/sec on modern CPUs, long prompts (such as 2000-token enterprise system prompts) introduce 25-40 seconds of prefill latency.

ClawMind solves this with an ultra-compact system prompt of approximately 180 tokens:
- Direct, concise tool specifications.
- Minimalist syntactic delimiters.
- Rolling history truncation preserving system rules while bounding past turns to the last 8 interactions.

This design reduces prefill latency on standard Intel/AMD desktop CPUs to under 1.0 second.

## 3. Tool Execution Layer

### 3.1 Shell Tool (`src/tools/shell.rs`)
- Platform-Aware Execution: Dispatches commands via `/bin/bash -c` on Linux and `/bin/zsh -c` on macOS.
- State Persistence: Tracks working directory state across invocations, supporting `cd` commands transparently.
- Safety Timeout: Enforces a hard 30-second execution deadline using asynchronous Tokio timeouts to prevent command hangs.
- Output Management: Captures both stdout and stderr, truncating excessively long logs (2000 characters maximum) to prevent memory and context blowout.

### 3.2 Filesystem Tool (`src/tools/fs.rs`)
- `file_write`: Creates or updates files, automatically creating missing parent directory hierarchies.
- `file_read`: Reads file contents with configurable line limits.
- `file_delete`: Removes target files or directories safely.
- `file_list`: Lists directory contents with item classification and file sizes in bytes.
- `file_analyze`: Inspects metadata, line count, word count, character count, and structural previews.

### 3.3 Browser Tool (`src/tools/browser.rs`)
- Automated Chrome Discovery: Locates Google Chrome binaries across standard Linux paths (`google-chrome`, `google-chrome-stable`, `chromium`) and macOS app bundles (`/Applications/Google Chrome.app`).
- Visible Navigation (`browser_open`): Spawns Chrome in dedicated windows, presenting live search and page content to the user.
- Search Integration (`browser_search`): Automatically constructs URL-encoded Google search queries.
- Headless Extraction (`browser_fetch`): Utilizes an internal high-speed HTTP retrieval and HTML tag-stripping engine to inspect web page text in under 500 milliseconds without browser rendering overhead.

### 3.4 Mouse and Keyboard Tool (`src/tools/mouse.rs`)
- Linux Support: Interacts with the desktop session using `xdotool` or `pyautogui` fallbacks for mouse movements, button clicks, text entry, and special key strokes.
- macOS Support: Interfaces directly with AppleScript System Events to dispatch native click, keystroke, and positioning events.

## 4. Hardware Orchestration and Inference Engine

### 4.1 Architecture-Aware Thread Steering
- Autoregressive Single-Token Decode: Bound to physical CPU cores to minimize L1/L2 cache misses and eliminate hyperthread competition.
- Prompt Batch Evaluation (Prefill): Utilizes the full logical thread allocation to maximize multi-threaded BLAS tensor computation.

### 4.2 Inactivity Hibernation
- When idle for more than `idle_timeout_secs` (default: 300 seconds), model weights and KV-cache allocations are cleanly dropped from RAM.
- Total memory drops from 4.2 GB to baseline operating footprint.
- CPU consumption during hibernation is verified at 0.0% through passive OpenMP thread sleep policies (`OMP_WAIT_POLICY=PASSIVE`).
- The worker thread awakens on the first incoming request and reloads the model automatically.

## 5. Deployment and Orchestration

The system is managed via `run.sh`:
- Automatic dependency and toolchain verification.
- Local model weight verification and HuggingFace acquisition.
- Optimized release compilation (`cargo build --release`).
- Zero background daemons: Runs purely on demand without occupying system RAM at boot.
- Supports both interactive agent mode and headless server mode via CLI arguments.
