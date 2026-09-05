#!/usr/bin/env bash
# ClawMind Interactive Agent Terminal
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "$SCRIPT_DIR/target/release/clawmind" agent "$@"
