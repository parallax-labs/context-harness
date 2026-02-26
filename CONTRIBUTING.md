# Contributing to Context Harness

Thanks for your interest in contributing!

## Good first issues

We tag small, well-scoped tasks with [**good first issue**](https://github.com/parallax-labs/context-harness/labels/good%20first%20issue). New contributors can also look for [**help wanted**](https://github.com/parallax-labs/context-harness/labels/help%20wanted).

**Current good first issues:** [#7](https://github.com/parallax-labs/context-harness/issues/7), [#8](https://github.com/parallax-labs/context-harness/issues/8).

## Getting Started

1. Fork the repository
2. Clone your fork
3. Create a feature branch: `git checkout -b feature/my-feature`
4. Make your changes
5. Run tests: `cargo test`
6. Run lints: `cargo clippy` and `cargo fmt`
7. Commit and push
8. Open a pull request

## Development Setup

```bash
# Clone
git clone https://github.com/parallax-labs/context-harness.git
cd context-harness

# Copy example config
cp config/ctx.example.toml config/ctx.toml

# Build
cargo build

# Test
cargo test

# Run
cargo run -- --help
```

## Code Style

- Run `cargo fmt` before committing
- All code must pass `cargo clippy` without warnings
- Write tests for new functionality
- Keep functions focused and well-documented

## Architecture

See the [docs/](docs/) directory for design documents:

- `DESIGN.md` — implementation design and module mapping
- `USAGE.md` — CLI contract and public interface
- `SCHEMAS.md` — JSON response schemas
- `PHASE_1_ACCEPTANCE.md` — Phase 1 acceptance criteria

## Reporting Issues

Please include:

- Steps to reproduce
- Expected behavior
- Actual behavior
- OS and Rust version

