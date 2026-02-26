+++
title = "Local Embeddings on Every Platform"
description = "Context Harness now ships local embeddings on all six release targets — no ORT install, no musl or Intel Mac left behind."
date = 2026-02-26

[taxonomies]
tags = ["release", "embeddings"]
+++

Context Harness has always supported **local** embeddings: fully offline, no API key, models downloaded on first use. The catch was that two of our six release targets — **Linux musl** and **macOS Intel** — couldn't ship with local embeddings because ONNX Runtime doesn't provide prebuilt binaries for them, and we didn't want to ask users to install ORT or set `ORT_DYLIB_PATH`. So those binaries only supported OpenAI and Ollama.

We've changed that. Every release binary now includes the local embedding provider. No system dependencies, no env vars, no special install steps.

## How we did it: two backends, one config

We kept the same config and API. Under the hood we now use **two** backends and pick the right one per target:

- **Primary platforms** (Linux glibc, Linux aarch64, macOS Apple Silicon, Windows): **fastembed** with ONNX Runtime **bundled at compile time** (download-binaries). Same fast path as before; zero user-managed deps.
- **Fallback platforms** (Linux musl, macOS Intel): a **pure-Rust** path using **tract-onnx** and the **tokenizers** crate. No native ORT, no C++ runtime; models still download from Hugging Face on first use and cache locally.

The release workflow selects the backend via Cargo features: default features for primary targets, `--no-default-features --features local-embeddings-tract` for musl and Intel Mac. Users get one binary per platform; they don't choose backends.

## What you get

| Binary | Local embeddings | Backend |
|--------|-------------------|---------|
| Linux x86_64 (glibc) | ✅ | fastembed |
| Linux x86_64 (musl) | ✅ | tract |
| Linux aarch64 | ✅ | fastembed |
| macOS x86_64 (Intel) | ✅ | tract |
| macOS aarch64 (Apple Silicon) | ✅ | fastembed |
| Windows x86_64 | ✅ | fastembed |

Same `provider = "local"` in config. Same model names (we support `all-minilm-l6-v2` on both backends; fastembed supports more). Same “model downloads on first use” behavior. No ORT install, no `ORT_DYLIB_PATH`, no Nix wrapper for ORT — the binary is self-contained.

## For Nix users

The flake default is now the **full** build (with local embeddings). `nix build` or `nix build .#default` gives you the full binary. For a minimal binary without local embeddings: `nix build .#no-local-embeddings`. On macOS we use **Zig** as the C/C++ compiler in the Nix environment so linking works without pulling in a separate libc++; no extra config needed.

## Summary

- **All six release targets** ship with local embeddings.
- **No system dependencies** for local: no ORT install, no env vars.
- **Two backends:** fastembed (primary) and tract (musl / Intel Mac), chosen at build time.
- **One config:** `provider = "local"` everywhere; same model semantics where both backends support them.

If you were on musl or Intel Mac and had to use Ollama or OpenAI only — you can now use `provider = "local"` with the official binaries and get fully offline embeddings with zero extra setup.
