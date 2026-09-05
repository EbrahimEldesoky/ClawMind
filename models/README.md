# ClawMind Models Directory

This directory stores quantized GGUF model files used for local inference.

Model files (`*.gguf`) are excluded from Git tracking due to their large size.

## Default Model
- Model: `gemma-4-E2B-it-Q4_K_M.gguf`
- Format: GGUF Quantized (Q4_K_M)

## Setup
When running `./run.sh`, the required model will be automatically verified or downloaded if not present.
Alternatively, place your custom GGUF models directly into this folder.
