#!/usr/bin/env bash
# Download model files and verify their checksums.
# Update backend/assets/models.toml with the correct sha256/size_bytes after download.
set -euo pipefail

MODEL_DIR="${MODEL_DIR:-/data/models}"

mkdir -p "$MODEL_DIR/llm"
mkdir -p "$MODEL_DIR/embeddings"
mkdir -p "$MODEL_DIR/whisper"

echo "==> Downloading Mistral 7B Instruct Q4_K_M..."
# Update this URL when the model is finalized
# curl -L -o "$MODEL_DIR/llm/mistral-7b-instruct-v0.2.Q4_K_M.gguf" \
#   "https://huggingface.co/TheBloke/Mistral-7B-Instruct-v0.2-GGUF/resolve/main/mistral-7b-instruct-v0.2.Q4_K_M.gguf"

echo "==> Downloading Nomic Embed Text v1.5..."
# curl -L -o "$MODEL_DIR/embeddings/nomic-embed-text-v1.5.f16.gguf" \
#   "https://huggingface.co/nomic-ai/nomic-embed-text-v1.5-GGUF/resolve/main/nomic-embed-text-v1.5.f16.gguf"

echo "==> Downloading Whisper base.en..."
# curl -L -o "$MODEL_DIR/whisper/ggml-base.en.bin" \
#   "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin"

echo ""
echo "After download, run the following to get checksums for models.toml:"
echo ""
echo "  sha256sum \$MODEL_DIR/llm/mistral-7b-instruct-v0.2.Q4_K_M.gguf"
echo "  sha256sum \$MODEL_DIR/embeddings/nomic-embed-text-v1.5.f16.gguf"
echo "  sha256sum \$MODEL_DIR/whisper/ggml-base.en.bin"
echo ""
echo "Update backend/assets/models.toml with the output, then restart the backend."
