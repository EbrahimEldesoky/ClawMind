#!/usr/bin/env bash
set -e

# ==============================================================================
# ClawMind: Ultra-Fast Autonomous AI Agent Orchestrator (Ubuntu & macOS)
# ==============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

MODEL_DIR="$SCRIPT_DIR/models"
MODEL_FILE="$MODEL_DIR/gemma-4-E2B-it-Q4_K_M.gguf"
MIN_MODEL_BYTES=3000000000
HF_MODEL_URL="https://huggingface.co/unsloth/gemma-4-E2B-it-GGUF/resolve/main/gemma-4-E2B-it-Q4_K_M.gguf"

CYAN='\033[0;36m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BOLD='\033[1m'
NC='\033[0m'

echo -e "${CYAN}══════════════════════════════════════════════════════════════════${NC}"
echo -e "       ${GREEN}${BOLD}🚀 ClawMind Autonomous AI Agent Orchestrator${NC}"
echo -e "       ${YELLOW}Hardware-Steered • Direct OS & Chrome Automation${NC}"
echo -e "${CYAN}══════════════════════════════════════════════════════════════════${NC}"

# 1. Environment and toolchain verification
echo -e "\n${CYAN}[1/4] Checking build environment & toolchain...${NC}"
command -v curl >/dev/null 2>&1 || { echo -e "${RED}[ERROR] curl is required but not installed.${NC}"; exit 1; }
command -v cargo >/dev/null 2>&1 || { echo -e "${RED}[ERROR] cargo (Rust) is required but not installed.${NC}"; exit 1; }
command -v rustc >/dev/null 2>&1 || { echo -e "${RED}[ERROR] rustc is required but not installed.${NC}"; exit 1; }

OS_TYPE="$(uname -s)"
echo -e "  • Operating System : ${GREEN}${OS_TYPE} ($(uname -m))${NC}"
echo -e "  • Rust Toolchain   : ${GREEN}$(rustc --version | cut -d' ' -f2)${NC}"

# Check Chrome presence
if command -v google-chrome >/dev/null 2>&1 || command -v google-chrome-stable >/dev/null 2>&1 || [ -d "/Applications/Google Chrome.app" ]; then
    echo -e "  • Google Chrome    : ${GREEN}Detected (Ready for automated browser tasks)${NC}"
else
    echo -e "  • Google Chrome    : ${YELLOW}Not found in standard paths (Install Chrome for browser automation)${NC}"
fi

# Check optional GUI automation helpers
if [ "$OS_TYPE" = "Linux" ]; then
    if command -v xdotool >/dev/null 2>&1; then
        echo -e "  • GUI Input Tool   : ${GREEN}xdotool detected (Mouse/Keyboard automation active)${NC}"
    else
        echo -e "  • GUI Input Tool   : ${YELLOW}xdotool not installed (Install via 'sudo apt install xdotool' for mouse automation)${NC}"
    fi
fi

# 2. Model verification and download
echo -e "\n${CYAN}[2/4] Verifying Gemma 4 local model weights...${NC}"
mkdir -p "$MODEL_DIR"
MODEL_VALID=0

if [ -f "$MODEL_FILE" ]; then
    CURRENT_SIZE=$(stat -c%s "$MODEL_FILE" 2>/dev/null || stat -f%z "$MODEL_FILE" 2>/dev/null || echo 0)
    if [ "$CURRENT_SIZE" -ge "$MIN_MODEL_BYTES" ]; then
        echo -e "  • Model Status     : ${GREEN}Verified existing model ($(awk "BEGIN {printf \"%.2f\", $CURRENT_SIZE/1073741824}") GB)${NC}"
        MODEL_VALID=1
    else
        echo -e "  • Model Status     : ${YELLOW}Incomplete file ($CURRENT_SIZE bytes). Re-acquiring...${NC}"
        rm -f "$MODEL_FILE"
    fi
fi

if [ "$MODEL_VALID" -eq 0 ]; then
    DOWNLOAD_SOURCE="$HOME/Downloads/gemma-4-E2B-it-Q4_K_M.gguf"
    if [ -f "$DOWNLOAD_SOURCE" ]; then
        SOURCE_SIZE=$(stat -c%s "$DOWNLOAD_SOURCE" 2>/dev/null || stat -f%z "$DOWNLOAD_SOURCE" 2>/dev/null || echo 0)
        if [ "$SOURCE_SIZE" -ge "$MIN_MODEL_BYTES" ]; then
            echo -e "  • Copying from     : ${GREEN}$DOWNLOAD_SOURCE${NC}"
            cp "$DOWNLOAD_SOURCE" "$MODEL_FILE"
            MODEL_VALID=1
        fi
    fi
fi

if [ "$MODEL_VALID" -eq 0 ]; then
    echo -e "  • Downloading      : ${YELLOW}Downloading Gemma 4 weights from HuggingFace...${NC}"
    curl -L -C - --progress-bar -o "$MODEL_FILE" "$HF_MODEL_URL"
    DOWNLOADED_SIZE=$(stat -c%s "$MODEL_FILE" 2>/dev/null || stat -f%z "$MODEL_FILE" 2>/dev/null || echo 0)
    if [ "$DOWNLOADED_SIZE" -lt "$MIN_MODEL_BYTES" ]; then
        echo -e "${RED}[ERROR] Download failed or produced incomplete file ($DOWNLOADED_SIZE bytes).${NC}"
        exit 1
    fi
    echo -e "  • Download Status  : ${GREEN}Completed successfully.${NC}"
fi

# 3. Build optimized release binary
echo -e "\n${CYAN}[3/4] Building optimized native ClawMind release binary...${NC}"
cargo build --release
BIN_PATH="$SCRIPT_DIR/target/release/clawmind"
if [ ! -f "$BIN_PATH" ]; then
    echo -e "${RED}[ERROR] Build failed: $BIN_PATH not found.${NC}"
    exit 1
fi
echo -e "  • Binary Status    : ${GREEN}Ready at $BIN_PATH${NC}"

# 4. Launch ClawMind
echo -e "\n${CYAN}[4/4] Launching ClawMind...${NC}"
export OMP_WAIT_POLICY=PASSIVE

# Execute binary passing all user-provided command-line arguments (default is Agent TUI)
exec "$BIN_PATH" "$@"
