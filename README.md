# ClawMind

ClawMind is an ultra-fast, hardware-steered autonomous AI agent written in Rust. It automatically inspects the underlying hardware on any machine (CPU cores, hyperthreads, vector instruction sets, RAM, and VRAM) and dynamically optimizes execution parameters to deliver high-throughput, low-latency local inference combined with native operating system and browser control.

Unlike conventional LLM wrappers, ClawMind is a self-contained, autonomous agent capable of executing terminal commands, managing files, controlling Google Chrome, and automating mouse/GUI actions directly on Linux (Ubuntu) and macOS without external runtime dependencies.

## Core Capabilities

- Universal Hardware Orchestrator: Dynamically adapts to Linux and macOS (Apple Silicon M-series and Intel). Detects CPU topology, vector units (AVX2, FMA, AVX-512, NEON), and GPU backends (Metal, CUDA, ROCm, Vulkan).
- Standalone Autonomous Agent: Operates as an independent, self-contained agent with native tool execution and an interactive formatted terminal interface (TUI).
- Operating System Control: Fast command execution across bash (Linux) and zsh/bash (macOS) with timeout safety, output management, and working directory tracking.
- Filesystem Management: Built-in capabilities to create, read, write, delete, list, and analyze files with structure inspection and preview.
- Google Chrome Automation: Direct browser control to open URLs, trigger web searches, and perform fast headless page text extraction.
- Mouse and GUI Automation: Controls mouse movement, click events, text typing, and key presses on Linux (via X11/xdotool) and macOS (via System Events).
- Low-Latency ReAct Execution: Compact, token-efficient system prompting engineered specifically for local models on CPU, minimizing prefill latency while maintaining reliable tool calling.
- Smart Inactivity Hibernation: Automatically unloads model weights and context memory after a configurable idle period (default: 300 seconds), releasing RAM and dropping idle CPU usage to 0.0%.
- Native HTTP Server Mode: Optional OpenAI-compatible (`/v1/chat/completions`) and Ollama-compatible (`/api/tags`) server for external integrations.

## System Requirements

- Operating System: Linux (Ubuntu/Debian, x86_64, aarch64) or macOS (Apple Silicon M1-M4, Intel).
- Toolchain: Rust 1.80+ (`cargo`, `rustc`), `cmake`, C/C++ compiler (`gcc` or `clang`).
- Memory: Minimum 8 GB RAM (16 GB recommended).
- Optional: Google Chrome (for browser automation), `xdotool` on Linux (for mouse automation).

## Quick Start

Execute the automated setup script to build and launch ClawMind in interactive agent mode:

```bash
chmod +x run.sh
./run.sh
```

The script performs the following operations:
1. Validates build dependencies, toolchains, and optional tools (Chrome, xdotool).
2. Verifies or acquires the quantized model weights (`gemma-4-E2B-it-Q4_K_M.gguf`).
3. Compiles the binary with maximum compiler optimizations (`opt-level = 3`, `lto = fat`).
4. Launches the ClawMind autonomous interactive agent terminal.

## Execution Modes

### 1. Interactive Agent Mode (Default)

Launch the interactive agent terminal:

```bash
./target/release/clawmind agent
```

Or simply:

```bash
./run.sh
```

Built-in agent terminal commands:
- `/help`: Display available commands and task suggestions.
- `/tools`: List active autonomous tools and syntax.
- `/cwd`: Show or verify current working directory.
- `/status`: Show hardware metrics, engine status, and thread configuration.
- `/clear`: Clear terminal screen and show banner.
- `/reset`: Reset conversation history.
- `/exit`: Exit interactive agent session.

Example user requests inside the agent terminal:
- `open chrome and search for movie called it`
- `create a file hello.py that prints system info and run it`
- `list all files in the current folder with sizes`
- `read Cargo.toml and summarize the dependencies`
- `check available disk space and memory`

### 2. HTTP Server Mode

To run ClawMind as an OpenAI-compatible API server on port 8080:

```bash
./run.sh --server
```

Or directly via binary:

```bash
./target/release/clawmind server --port 8080 --host 127.0.0.1
```

Available server endpoints:
- `GET  /health`: Health status.
- `GET  /system/hardware`: Realtime hardware detection profile and telemetry.
- `GET  /v1/models`: OpenAI-compatible model listing.
- `POST /v1/chat/completions`: Streaming SSE and non-streaming chat completions.
- `GET  /api/tags`: Ollama-compatible model tag emulation.

### 3. Hardware Benchmark Mode

To run an end-to-end inference benchmark and measure tokens per second:

```bash
./run.sh --bench
```

## Tool Architecture

ClawMind includes a native Rust tool registry:

| Tool Name | Parameters | Description |
|---|---|---|
| `bash` | `<command>` | Executes shell command with 30s timeout and output capture |
| `file_write` | `{"path": "...", "content": "..."}` | Creates or overwrites files, ensuring parent directories |
| `file_read` | `{"path": "...", "max_lines": N}` | Reads file safely with optional line limits |
| `file_delete` | `{"path": "..."}` | Deletes target file or directory safely |
| `file_list` | `{"path": "..."}` | Lists directory items with size and type breakdown |
| `file_analyze` | `{"path": "..."}` | Analyzes file type, line count, word count, and structure |
| `browser_search` | `<query>` | Launches Google Chrome with Google search results |
| `browser_open` | `<url>` | Opens specific URL visibly in Google Chrome |
| `browser_fetch` | `<url>` | Fast headless HTTP text extraction without GUI |
| `script_list` | *(none)* | Discovers and indexes all client automation scripts in `scripts/` |
| `script_inspect` | `<name>` | Reads script source code, documentation tags, and parameters |
| `script_run` | `{"name": "...", "args": ["..."]}` | Executes automation script with safety checks and output capture |
| `mouse_click` | `{"x": 100, "y": 200, "button": 1}` | Moves and clicks mouse button at coordinates |
| `mouse_move` | `{"x": 100, "y": 200}` | Moves mouse pointer to coordinates |
| `mouse_type` | `{"text": "..."}` | Types text into active application window |
| `mouse_key` | `{"key": "Return"}` | Presses special keyboard keys |

## Script-RAG Automation Engine

ClawMind includes a specialized autonomous scripting engine tailored for client automation workflows:
- Place any Bash, Python, or executable script into the `scripts/` folder.
- ClawMind dynamically indexes script headers, descriptions, arguments, and usage patterns.
- Ask ClawMind in natural language (Arabic or English) to run, inspect, or create custom automation workflows on demand.

## License

Apache-2.0 or MIT.
