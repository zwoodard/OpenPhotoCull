# OpenPhotoCull

A fast, open-source desktop app for photo culling. Scan a folder, automatically analyze every image for quality issues, and quickly decide what to keep or delete.

Built with Rust + Tauri v2 + React. Runs natively on macOS with cross-platform support planned for Windows and Linux.

## Features

### Analysis Pipeline (single-pass, ~120 images/sec)

- **Blur detection** — Laplacian variance with tunable threshold. Distinguishes overall blur from intentional bokeh via subject focus detection.
- **Subject focus detection** — Computes blur separately in face regions vs. background. Flags back-focused shots (face blurry, background sharp) while correctly ignoring shallow depth-of-field.
- **Exposure analysis** — Luminance histogram detects underexposed, overexposed, and high-contrast images.
- **Duplicate detection** — EXIF timestamp clustering + perceptual hashing (dHash) groups near-identical shots together.
- **Closed eye detection** — Apple Vision framework detects faces and measures eye openness. Flags photos where someone blinked. *(macOS only)*
- **Face grouping by person** — Vision framework feature print embeddings clustered to identify unique people. Filter your library by person. *(macOS only)*
- **Scene grouping** — Groups temporally proximate photos into scenes with visual dividers in the grid.

### Review UI

- Virtualized photo grid (handles 10,000+ images smoothly)
- Side-by-side comparison mode for duplicate groups with "Pick Best" auto-selection
- Keyboard-driven workflow: arrow keys to navigate, K/D/U to mark keep/delete/unmark, C for comparison
- Filter by: blurry, exposure issues, closed eyes, back-focused, duplicates, person
- Bulk operations: select all filtered, auto-suggest deletions, bulk mark
- Tunable thresholds via Settings panel — changes apply instantly, no re-scan needed
- Deletions move files to OS trash (recoverable)

### Performance

Benchmarked on Apple Silicon (M-series) with 85 mixed photos (577 MB, JPEGs up to 16MB/26MP):

| Metric | Value |
|---|---|
| Processing speed | **121 images/sec** |
| Total scan+analyze time | **702ms** (85 photos) |
| Peak memory | ~40MB (constant, regardless of library size) |
| Largest JPEG decode | 65ms (16MB, 6240x4160 via Apple Image I/O) |

Key optimizations:
- Apple Image I/O hardware JPEG decoder with built-in Lanczos downscaling (2.5x faster than libjpeg-turbo)
- DCT-scaled JPEG decoding via turbojpeg (decode at 1/4 resolution)
- SIMD-accelerated image resizing via `fast_image_resize` (NEON on Apple Silicon)
- Single-pass pipeline: decode once, run all analyzers, drop immediately
- Constant memory per thread — no accumulation regardless of photo count

## Getting Started

### Prerequisites

- [Rust](https://rustup.rs/) (1.80+)
- [Node.js](https://nodejs.org/) (18+)
- [cmake](https://cmake.org/) (for turbojpeg build)
- macOS: Xcode command line tools (for Apple framework access)

```bash
# macOS
brew install cmake

# Install dependencies
npm install
```

### Development

```bash
npx tauri dev
```

### Production Build

```bash
npx tauri build
```

Binary is output at `src-tauri/target/release/photo-scrub`.

### Benchmarking

```bash
# Build the benchmark binary
cargo build --release --manifest-path src-tauri/Cargo.toml --bin bench

# Run on a test folder
./src-tauri/target/release/bench <folder_path> [threads]

# Example
./src-tauri/target/release/bench test-photos/sampled2 8
```

The benchmark reports per-phase timing, per-image breakdown (slowest/fastest), time distribution histogram, and a detailed single-image step breakdown (decode, resize, blur, exposure, phash).

### Generating Synthetic Test Images

```bash
cargo build --release --manifest-path src-tauri/Cargo.toml --bin gen_test_images
./src-tauri/target/release/gen_test_images <source.jpg> <output_dir>
```

Creates images with controlled blur levels (sharp → extreme) and exposure levels (very dark → very bright) for threshold tuning.

## Architecture

```
src-tauri/src/                    Rust backend
├── commands/
│   ├── scan.rs                   Single-pass pipeline: discovery → decode → analyze → group
│   ├── analyze.rs                Fetch pre-computed analysis results
│   ├── review.rs                 Keep/delete marks, bulk operations
│   └── export.rs                 Move marked files to OS trash
├── pipeline/
│   ├── blur.rs                   Laplacian variance blur detection
│   ├── exposure.rs               Luminance histogram exposure analysis
│   ├── duplicates.rs             EXIF clustering + perceptual hashing
│   ├── closed_eyes.rs            Apple Vision face/eye detection (macOS)
│   ├── face_grouping.rs          Vision feature print embeddings + clustering (macOS)
│   └── traits.rs                 Analyzer trait for extensibility
├── thumbnail/
│   ├── mod.rs                    Decode strategy: Image I/O → turbojpeg → image crate
│   └── apple_imageio.rs          Apple hardware JPEG decoder FFI (macOS)
├── index/
│   ├── discovery.rs              Recursive folder walk (walkdir)
│   ├── metadata.rs               EXIF extraction + orientation handling
│   └── store.rs                  Data types: IndexedImage, AnalysisResults, etc.
└── state.rs                      Thread-safe app state (Arc<RwLock>)

src/                              React/TypeScript frontend
├── components/
│   ├── FolderPicker.tsx           Landing screen with folder selection
│   ├── ScanProgress.tsx           Progress bar with timing debug panel
│   ├── ReviewLayout.tsx           Main review shell (grid + detail + comparison)
│   ├── PhotoGrid.tsx              Virtualized thumbnail grid with scene dividers
│   ├── PhotoDetail.tsx            Full-size preview + analysis details
│   ├── ComparisonView.tsx         Side-by-side duplicate comparison
│   ├── FilterBar.tsx              Filter chips with live counts
│   ├── PersonFilter.tsx           Face thumbnail chips for person filtering
│   ├── BulkActions.tsx            Bulk mark/delete toolbar
│   └── SettingsPanel.tsx          Tunable thresholds (blur, exposure, scene window)
├── store/
│   ├── index.ts                   Zustand store with filters, settings, derived data
│   └── types.ts                   TypeScript type definitions
└── lib/
    └── tauri.ts                   Typed wrappers for Tauri IPC commands
```

## Keyboard Shortcuts

| Key | Action |
|---|---|
| Arrow keys | Navigate photos |
| K | Mark selected as Keep |
| D | Mark selected as Delete |
| U | Unmark selected |
| C | Toggle comparison mode (duplicate groups) |
| Escape | Exit comparison mode |
| Shift+Click | Range select |
| Cmd/Ctrl+Click | Multi-select |

## Platform Support

| Feature | macOS | Windows | Linux |
|---|---|---|---|
| Core pipeline (blur, exposure, duplicates) | Yes | Yes | Yes |
| Apple Image I/O hardware decode | Yes | — | — |
| Closed eye detection (Vision) | Yes | — | — |
| Face grouping by person (Vision) | Yes | — | — |
| Subject focus detection | Yes* | Yes | Yes |
| Move to OS trash | Yes | Yes | Yes |

*Subject focus requires face bounding boxes from Vision on macOS. On other platforms, it falls back to overall blur only.

## Tech Stack

- **Rust** — image processing, analysis pipeline, file management
- **Tauri v2** — native desktop shell, IPC, asset protocol
- **React 18** — UI components
- **TypeScript** — type-safe frontend
- **Zustand** — state management
- **@tanstack/react-virtual** — virtualized grid rendering

### Key Rust Crates

| Crate | Purpose |
|---|---|
| `image` | Image decoding (PNG, TIFF, WebP) |
| `turbojpeg` | SIMD JPEG decode with DCT scaling |
| `fast_image_resize` | NEON/AVX2 image resizing |
| `kamadak-exif` | EXIF metadata extraction |
| `image_hasher` | Perceptual hashing for duplicates |
| `rayon` | Parallel processing |
| `objc2` | Apple framework FFI (Vision, Core Image) |
| `trash` | Cross-platform move-to-trash |
| `walkdir` | Recursive directory traversal |

## License

MIT
