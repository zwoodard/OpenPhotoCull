# CLAUDE.md

## Project Overview

OpenPhotoCull is a Tauri v2 desktop app (Rust backend + React/TypeScript frontend) for photo culling. Users select a folder, the app scans and analyzes all images in a single pass, then presents a review UI for bulk keep/delete decisions.

## Quick Commands

```bash
# Development
npx tauri dev

# Production build
npx tauri build

# Type-check frontend only
npx tsc --noEmit

# Check Rust only
cargo check --manifest-path src-tauri/Cargo.toml

# Run benchmark (bench/dev binaries are gated behind the bench-bins feature)
cargo build --release --manifest-path src-tauri/Cargo.toml --features bench-bins --bin bench
./src-tauri/target/release/bench test-photos/sampled2 8

# Generate synthetic test images
cargo build --release --manifest-path src-tauri/Cargo.toml --features bench-bins --bin gen_test_images
./src-tauri/target/release/gen_test_images test-photos/sampled2/DSCF9687.JPG test-photos/synthetic
```

## Architecture Essentials

### Data Flow

```
Folder → discovery (walkdir) → parallel per-image processing (rayon, 8 threads):
  → EXIF extraction (header only)
  → Decode (Image I/O on macOS > 1MB, turbojpeg otherwise)
  → SIMD resize to 1024px (fast_image_resize)
  → Thumbnail from resized image (300px JPEG)
  → Blur detection (Laplacian variance)
  → Exposure analysis (luminance histogram)
  → Perceptual hash (dHash 16x16)
  → Face detection + eye openness (Apple Vision, macOS)
  → Subject focus (blur in face vs background regions)
  → Face embeddings for person grouping (Apple Vision, macOS)
  → DROP analysis image (constant memory)
→ Post-pass grouping (timestamps only, no images):
  → Duplicate grouping (hash similarity within 5s time clusters)
  → Scene grouping (60s time window)
  → Person clustering (L2 distance on face embeddings)
→ Store results in AppState (Arc<RwLock>)
→ Frontend fetches via Tauri IPC commands
```

### Key File Map

| What you want to change | File(s) |
|---|---|
| Add a new analyzer | `src-tauri/src/commands/scan.rs` (pipeline loop), `src-tauri/src/index/store.rs` (types) |
| Add a new Tauri command | `src-tauri/src/commands/`, register in `src-tauri/src/lib.rs` |
| Change decode strategy | `src-tauri/src/thumbnail/mod.rs` (process_image fn) |
| Apple framework FFI | `src-tauri/src/thumbnail/apple_imageio.rs`, `src-tauri/src/pipeline/closed_eyes.rs`, `src-tauri/src/pipeline/face_grouping.rs` |
| Add frontend filter | `src/store/index.ts` (filters + filteredImages), `src/components/FilterBar.tsx` |
| Add analysis badge | `src/components/PhotoGrid.tsx` (badge section near line 250) |
| Add detail panel info | `src/components/PhotoDetail.tsx` (analysis section) |
| Change grid layout | `src/components/PhotoGrid.tsx` (COLUMN_COUNT, ROW_HEIGHT) |
| Change thumbnail cache | `src-tauri/src/thumbnail/mod.rs` (thumbnail_path fn, cache key) |

### State Shape (Zustand)

```typescript
{
  images: ImageEntry[]                    // All discovered images
  analysisMap: Record<id, AnalysisResults> // Per-image analysis scores
  duplicateGroups: Record<groupId, id[]>  // Duplicate clusters
  sceneGroups: Record<sceneId, id[]>      // Scene clusters
  personGroups: Record<personId, [{imageId, faceIndex}]>
  marks: Record<id, "keep"|"delete"|"unmarked">
  settings: { blurThreshold, exposureThreshold, duplicateThreshold, sceneWindowSecs }
  filters: { showBlurry, showExposureIssues, showClosedEyes, showBackFocus, filterByPersonId, showDuplicatesOnly, sortBy }
}
```

### Rust Analysis Types (store.rs)

```rust
AnalysisResults {
  blur: Option<BlurResult>           // laplacian_variance, is_blurry
  exposure: Option<ExposureResult>   // mean_luminance, pct_under/over, verdict
  duplicate_group_id: Option<String> // "dup-1", "dup-2", etc.
  scene_group_id: Option<String>     // "scene-1", etc.
  closed_eyes: Option<ClosedEyesResult> // face_count, per-face eye openness
  subject_focus: Option<SubjectFocusResult> // subject vs bg blur, verdict
  faces: Option<Vec<FaceInfo>>       // per-face bbox, person_id, thumbnail
}
```

## Important Patterns

### Platform Gating

macOS-specific code (Vision framework, Image I/O) is gated with `#[cfg(target_os = "macos")]`. Every macOS-only function has a non-macOS stub that returns `None` or empty results. The app compiles and runs on all platforms — macOS features just won't be available.

### Memory Management

The pipeline is designed for constant memory regardless of photo count. Each rayon thread decodes one image at a time, runs all analyzers, then drops the image. Never accumulate decoded images across threads. The `into_rgba8()` pattern (consuming, not copying) is critical.

### Thumbnail Cache

Thumbnails are cached at `~/Library/Caches/com.openphotocull.OpenPhotoCull/thumbs/` (macOS). Cache key is `SHA256(path + modified_time)`. The `_v2.jpg` suffix was added to invalidate pre-orientation-fix thumbnails.

### Frontend Filtering

Threshold tuning (blur, exposure) is done entirely on the frontend — the backend stores raw continuous scores, and `isBlurry()`, `hasExposureIssue()` etc. apply thresholds at display time. This means threshold changes are instant with no re-scan.

### Apple Vision FFI

The `objc2` crate handles ObjC message dispatch. For struct returns (like CGRect from `boundingBox`), we use raw `objc_msgSend` function pointer casts because `objc2`'s `msg_send!` requires `Encode` impl for the return type. See `closed_eyes.rs` bounding box extraction for the pattern.

## Test Data

- `test-photos/sampled2/` — 85 real photos (mixed JPEG, PNG, CR2), ~577MB
- `test-photos/synthetic/` — 16 generated images with controlled blur/exposure levels
- Benchmark binary reports per-image timing, throughput, and detailed decode breakdown

## Performance Baseline

With 85 photos on Apple Silicon:
- Pipeline: 121 imgs/sec (702ms total)
- Cached run: 131 imgs/sec (646ms)
- Slowest single image: ~126ms (16MB JPEG via Image I/O)
- EXIF extraction: ~8000 imgs/sec
