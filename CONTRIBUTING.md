# Contributing to OpenPhotoCull

Thanks for your interest in contributing! This guide will help you get started.

## Development Setup

### Prerequisites

- [Rust](https://rustup.rs/) (1.80+)
- [Node.js](https://nodejs.org/) (18+)
- [cmake](https://cmake.org/) (required for turbojpeg)
- macOS: Xcode command line tools (`xcode-select --install`)

### Getting Started

```bash
# Clone the repo
git clone https://github.com/YOUR_USERNAME/photo-scrub.git
cd photo-scrub

# Install frontend dependencies
npm install

# Run in development mode (hot-reload frontend + Rust backend)
npx tauri dev

# Type-check frontend
npx tsc --noEmit

# Check Rust compilation
cargo check --manifest-path src-tauri/Cargo.toml
```

## Project Structure

- `src/` — React/TypeScript frontend (components, store, IPC wrappers)
- `src-tauri/src/` — Rust backend (analysis pipeline, commands, image processing)
- See `CLAUDE.md` for a detailed architecture guide and key file map

## Making Changes

### Adding a New Analyzer

1. For Vision/FFI-backed analyzers, create a module in `src-tauri/src/pipeline/` (see `saliency.rs` or `closed_eyes.rs` for the FFI pattern). For pure-Rust analyzers, just add a function alongside `compute_blur` / `compute_exposure` in `src-tauri/src/commands/scan.rs`.
2. Add result types to `src-tauri/src/index/store.rs` (`AnalysisResults` struct)
3. Wire it into the scan pipeline in `src-tauri/src/commands/scan.rs`
4. Add TypeScript types in `src/store/types.ts`
5. Add UI (badge in `PhotoGrid.tsx`, details in `PhotoDetail.tsx`, filter in `FilterBar.tsx`)

### Adding a Frontend Filter

1. Add the filter field to `filters` in `src/store/index.ts`
2. Add filtering logic in the `filteredImages` selector
3. Add a chip in `src/components/FilterBar.tsx`

### Platform-Specific Code

macOS-only features (Vision framework, Image I/O) must be gated with `#[cfg(target_os = "macos")]` and have a stub for other platforms. The app should always compile and run on all platforms — macOS features just won't be available.

## Code Style

- **Rust**: Follow standard Rust conventions. `cargo fmt` and `cargo clippy` should pass cleanly.
- **TypeScript**: Standard React patterns. No specific linter configured yet — just be consistent with the existing code.
- **Keep it simple**: Avoid over-engineering. The codebase is intentionally lean. Don't add abstractions for one-off operations.

## Performance

This project prioritizes performance. When making changes to the analysis pipeline:

- Run the benchmark before and after: `cargo build --release --manifest-path src-tauri/Cargo.toml --bin bench && ./src-tauri/target/release/bench test-photos/sampled2 8`
- Watch for memory — the pipeline is designed for constant memory per thread. Never accumulate decoded images.
- Prefer consuming operations (`into_rgba8()`) over cloning.

## Pull Requests

1. Fork the repo and create a branch from `main`
2. Make your changes with clear, focused commits
3. Ensure `npx tsc --noEmit` and `cargo check` pass
4. Describe what your PR does and why in the description
5. Include before/after screenshots for UI changes
6. Include benchmark numbers for pipeline changes

## Reporting Issues

File issues on GitHub with:
- What you expected vs. what happened
- Steps to reproduce
- Your OS and version
- Any relevant logs (Rust backend logs go to stderr)

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
