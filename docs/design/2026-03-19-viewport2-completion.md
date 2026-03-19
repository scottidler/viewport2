# Design Document: viewport2 Completion

**Author:** Scott A. Idler
**Date:** 2026-03-19
**Status:** Implemented
**Review Passes Completed:** 5/5

## Summary

Close the remaining gaps between the original viewport2 design doc (2026-03-18) and the current implementation. Four items: extract `pipeline.rs` from inlined logic in `main.rs`, add edge/corner resize drag handles to the undecorated overlay window, document the always-on-top workflow (GTK4 on Wayland has no programmatic API), and replace the manual scalar BGRx-to-YUYV conversion with SIMD-accelerated `yuvutils-rs`.

## Problem Statement

### Background

The original viewport2 design doc specified five phases. All five are implemented and shipped as v0.1.0, but an audit revealed four gaps where the implementation diverges from the spec.

### Problem

1. **No `pipeline.rs` module** - the architecture diagram and module breakdown specify a dedicated `pipeline.rs` that ties capture + convert + output together. The logic currently lives inline in `main.rs` (lines 125-201), making main.rs harder to reason about and test.

2. **No edge/corner resize handles** - the design says "Resizable via edge/corner drag handles." The current overlay sets `resizable(true)` but since the window is undecorated, there are no visible or functional grab handles. Users can only resize via Ctrl+Arrow keyboard shortcuts.

3. **No always-on-top** - the design says "Always-on-top via GTK4 window hints." GTK4 removed `set_keep_above()` and Wayland provides no client-side API for this. The overlay can disappear behind other windows.

4. **No SIMD conversion** - the design specifies `yuvutils-rs` for SIMD-accelerated BGRx-to-YUYV conversion. The current implementation uses manual scalar BT.601 integer math, which works but is slower on large frames (4K source).

### Goals

- Extract pipeline orchestration into `pipeline.rs` matching the architecture diagram
- Add mouse-driven edge/corner resize via `GdkToplevel::begin_resize()`
- Document the always-on-top user workflow in README and add a startup hint
- Replace manual conversion with `yuvutils-rs` SIMD path
- Maintain existing test coverage and add new tests

### Non-Goals

- Implementing a GNOME Shell extension for programmatic always-on-top
- Forcing X11 backend via `GDK_BACKEND=x11`
- Adding `gtk4-layer-shell` (not supported on GNOME/Mutter)
- Bilinear or bicubic resize (nearest-neighbor is sufficient)

## Proposed Solution

### Overview

Four independent changes that can be implemented in sequence. Each is self-contained and testable.

### Phase 1: Extract `pipeline.rs`

Move the frame processing loop from `main.rs` into a new `pipeline.rs` module.

**New file: `src/pipeline.rs`**

```rust
pub struct PipelineConfig {
    pub device: String,
    pub output_width: u32,
    pub output_height: u32,
    pub target_fps: u32,
}

pub fn run(
    config: PipelineConfig,
    frame_rx: mpsc::Receiver<Frame>,
    shared_rect: Arc<AtomicRect>,
) {
    // Open v4l2 device
    // Frame loop: recv -> throttle -> crop -> resize -> convert -> write
}
```

**Changes to `main.rs`:**
- Add `mod pipeline;`
- Replace the inline output thread spawn (lines 125-201) with:
  ```rust
  let pipeline_config = pipeline::PipelineConfig { ... };
  let output_handle = std::thread::Builder::new()
      .name("v4l2-output".into())
      .spawn(move || pipeline::run(pipeline_config, frame_rx, output_rect))
      .context("Failed to spawn pipeline thread")?;
  ```

This is a pure refactor - no behavioral change. The test suite validates nothing breaks.

### Phase 2: Edge/Corner Resize Handles

Replace the current drag gesture (move-only) with an edge-aware gesture that calls either `begin_move()` or `begin_resize()` depending on cursor proximity to the window border.

**Constants:**
```rust
const EDGE_THRESHOLD: f64 = 8.0; // pixels from edge to trigger resize
```

**Edge detection helper:**
```rust
fn detect_edge(x: f64, y: f64, width: f64, height: f64, threshold: f64) -> Option<gdk::SurfaceEdge>
```

Compares cursor position against window dimensions. Returns `Some(edge)` when within threshold of an edge/corner, `None` for interior (move zone).

**Cursor feedback via `EventControllerMotion`:**
- Track cursor position on every motion event
- Store the detected edge in `Rc<Cell<Option<SurfaceEdge>>>`
- Set appropriate resize cursor (`"nw-resize"`, `"n-resize"`, etc.) or `"default"` for interior

**Edge-aware drag gesture:**
- On `drag_begin`, read the stored edge state
- If `Some(edge)`: call `toplevel.begin_resize(edge, Some(&device), 1, 0.0, 0.0, 0)`
- If `None`: call `toplevel.begin_move(&device, 1, 0.0, 0.0, 0)` (existing behavior)

**Resize sync to shared rect:**
- The existing `connect_default_width_notify` handler already syncs size to `shared_rect`
- Add `connect_default_height_notify` for height changes (currently missing)

**Unit tests:**
- `test_detect_edge_corners` - all four corners
- `test_detect_edge_sides` - all four edges
- `test_detect_edge_interior` - returns None for center

### Phase 3: Always-on-Top Documentation

GTK4 on Wayland has no programmatic API for always-on-top. The `set_keep_above()` method from GTK3 was removed. GNOME/Mutter does not implement `wlr-layer-shell`. The only options are:

1. **User action** (chosen): Super+right-click window -> "Always on Top" in GNOME
2. GNOME Shell extension (too heavy for this tool)
3. Force X11 backend (loses Wayland benefits)

**Changes:**
- Update README "Usage" section to include the always-on-top step
- Print a one-time hint to stderr on startup: `"Tip: Super+right-click the viewport2 window and select 'Always on Top'"` - only shown when no config file exists yet (first run)
- Update the original design doc's risk table to reflect the actual Wayland limitation instead of claiming `set_keep_above()` works

### Phase 4: SIMD Conversion via yuvutils-rs

Replace the manual `bgrx_to_yuyv()` function with `yuvutils-rs` which provides SIMD-accelerated conversion (SSE4.1/AVX2 on x86_64, NEON on aarch64).

**Dependency:**
```toml
yuvutils-rs = "0.8"
```

**Conversion path** (yuvutils-rs has no direct BGRx-to-YUYV function):
1. `bgra_to_yuv422()` - BGRx/BGRA -> YUV 422 planar (SIMD)
2. `yuv422_to_yuyv422()` - YUV 422 planar -> YUYV packed (SIMD)

BGRx has identical byte layout to BGRA (the 4th byte is padding), so `bgra_to_yuv422` works directly on BGRx buffers.

**Changes to `convert.rs`:**
- Replace `bgrx_to_yuyv()` body with yuvutils-rs calls
- Pre-allocate the intermediate YUV 422 planar buffer once per pipeline run (not per frame) via a `Converter` struct that holds the reusable buffer
- Keep `crop_bgrx()` and `resize_bgrx_nearest()` unchanged

**New struct in `convert.rs`:**
```rust
pub struct Converter {
    planar: YuvPlanarImageMut<'static, u8>,
    width: u32,
    height: u32,
}

impl Converter {
    pub fn new(width: u32, height: u32) -> Self { ... }
    pub fn bgrx_to_yuyv(&mut self, src: &[u8], stride: u32, dst: &mut [u8]) { ... }
}
```

**Pipeline integration:**
- Create `Converter` once at pipeline startup with the output resolution
- Call `converter.bgrx_to_yuyv()` instead of the free function
- Remove the old manual `bgrx_to_yuyv()` free function

**Existing tests:**
- `test_bgrx_to_yuyv_black`, `test_bgrx_to_yuyv_white`, `test_bgrx_to_yuyv_dimensions` - keep and adapt to use the new `Converter` struct
- Values may shift slightly due to different rounding in yuvutils-rs vs manual math. Update expected ranges if needed (both are BT.601 limited range, so within +/-1).

## Alternatives Considered

### Alternative 1: Keep manual scalar conversion
- **Description:** Leave the hand-written BT.601 loop as-is
- **Pros:** No new dependency, code is simple and readable, already correct
- **Cons:** ~3-5x slower than SIMD at 4K resolution. At 1280x720 the difference is negligible (~0.5ms manual vs ~0.15ms SIMD), but 4K source frames are cropped before conversion so the actual input to convert is always output_size (1280x720)
- **Why not chosen:** The original design spec calls for yuvutils-rs. The performance gain matters if output_size is ever increased to 1920x1080 or higher. Adding the dependency now is low risk.

### Alternative 2: gtk4-layer-shell for always-on-top
- **Description:** Use the wlr-layer-shell protocol to place the overlay on the compositor's overlay layer
- **Pros:** Programmatic always-on-top, no user action needed
- **Cons:** GNOME/Mutter does not implement wlr-layer-shell. Only works on wlroots (Sway, Hyprland) and Smithay (COSMIC) compositors.
- **Why not chosen:** viewport2 targets GNOME/Mutter. This approach would break on the target platform.

### Alternative 3: GNOME Shell extension for always-on-top
- **Description:** Ship a minimal GNOME Shell extension that watches for the viewport2 window and calls `meta_window.make_above()`
- **Pros:** Fully automatic, no user interaction
- **Cons:** Requires separate install step, GNOME extension review, version-locked to GNOME Shell releases, adds maintenance burden
- **Why not chosen:** Too heavy for a single feature. The user action (Super+right-click) is a one-time step.

### Alternative 4: Inline pipeline in main.rs (keep current structure)
- **Description:** Don't extract pipeline.rs, leave the frame loop in main.rs
- **Pros:** One fewer file, no refactor needed
- **Cons:** main.rs is 212 lines with mixed concerns (CLI, logging, preflight, session setup, frame processing). The original architecture explicitly specifies pipeline.rs as a separate module.
- **Why not chosen:** Matching the architecture diagram improves readability and makes the pipeline independently testable.

## Technical Considerations

### Dependencies

**New:**
- `yuvutils-rs = "0.8"` - SIMD pixel format conversion (SSE4.1/AVX2/NEON, pure Rust fallback)

**Existing (unchanged):**
- All current Cargo.toml dependencies remain

### Performance

- SIMD conversion: ~0.15ms for 1280x720 (vs ~0.5ms manual). At 30fps, saves ~10ms/s of CPU time.
- Edge detection runs on every mouse motion event - trivial cost (4 comparisons, no allocation)
- Pipeline extraction: no performance change (same code, different file)

### Security

No new security considerations. The yuvutils-rs crate uses `core::arch` SIMD intrinsics internally (unsafe blocks within the crate), but its public API is safe Rust. No new unsafe code in viewport2.

### Testing Strategy

- **Phase 1 (pipeline.rs):** Existing 17 tests pass unchanged. No new tests needed - this is a pure move refactor.
- **Phase 2 (resize handles):** 3 new unit tests for `detect_edge()`. Manual testing for visual resize behavior.
- **Phase 3 (always-on-top docs):** No tests - documentation only.
- **Phase 4 (yuvutils-rs):** Adapt 3 existing conversion tests to use new `Converter` struct. Verify black/white/gray produce correct YUV values within +/-2 tolerance.

### Rollout Plan

1. Implement phases 1-4 in sequence
2. `otto ci` after each phase
3. Commit each phase separately
4. Bump version to v0.2.0 (minor - adds visible behavior change: resize handles)
5. `git push && git push --tags`
6. `cargo install --path .`

## Implementation Plan

### Phase 1: Extract pipeline.rs
- Create `src/pipeline.rs` with `PipelineConfig` struct and `run()` function
- Move frame loop from `main.rs` into `pipeline::run()`
- Update `main.rs` to call `pipeline::run()` via thread spawn
- **Validate:** `otto ci` passes, 17 tests green, no behavioral change

### Phase 2: Edge/corner resize handles
- Add `detect_edge()` helper and cursor mapping to `overlay.rs`
- Add `EventControllerMotion` for cursor tracking and visual feedback
- Replace move-only `GestureDrag` with edge-aware drag (move or resize)
- Add `connect_default_height_notify` for height sync
- Add 3 unit tests for edge detection
- **Validate:** `otto ci` passes, overlay resizes from edges/corners

### Phase 3: Always-on-top documentation
- Update README with always-on-top user instruction
- Add first-run stderr hint in `main.rs`
- Update original design doc risk table
- **Validate:** `otto ci` passes, README is accurate

### Phase 4: SIMD conversion via yuvutils-rs
- Add `yuvutils-rs = "0.8"` to Cargo.toml
- Create `Converter` struct in `convert.rs` with pre-allocated planar buffer
- Replace manual `bgrx_to_yuyv()` with yuvutils-rs two-step path
- Update `pipeline.rs` to use `Converter`
- Adapt existing conversion tests
- **Validate:** `otto ci` passes, `ffplay /dev/video10` shows correct colors

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| yuvutils-rs produces slightly different YUV values than manual math | High | Low | Both use BT.601 limited range. Values differ by at most +/-1 due to rounding. Adjust test tolerances. |
| begin_resize() not working on some Wayland compositors | Low | Medium | GTK4 delegates to compositor. Works on GNOME/Mutter (tested). Keyboard resize remains as fallback. |
| yuvutils-rs intermediate planar buffer doubles memory for conversion | Low | Low | At 1280x720: planar buffer is ~1.4MB. Allocated once, reused every frame. Negligible. |
| Edge detection interferes with drag-to-move | Low | Medium | Interior zone (>8px from edges) triggers move. Border zone triggers resize. Threshold is configurable. |
| EDGE_THRESHOLD smaller than border_width | Low | Low | Derive threshold from `max(border_width + 4, 8)` so the resize zone always covers the visible border. |
| Odd output width breaks YUYV packing | Low | Medium | Add validation in config loading: round output_size width up to nearest even number, log a warning if adjusted. |

## Open Questions

- [ ] Should `EDGE_THRESHOLD` be configurable via config file, or is 8px a good universal default?
- [ ] Should the first-run always-on-top hint be printed to stderr or shown as a GTK4 toast notification?

## References

- [yuvutils-rs crate](https://crates.io/crates/yuvutils-rs)
- [GTK4 always-on-top removal discussion](https://discourse.gnome.org/t/gtk-4-how-to-replace-gtk-window-set-keep-above-and-gtk-window-set-keep-below/3550)
- [gdk4::ToplevelExt::begin_resize](https://gtk-rs.org/gtk4-rs/stable/latest/docs/gdk4/struct.Toplevel.html)
- [gdk4::SurfaceEdge](https://gtk-rs.org/gtk4-rs/stable/latest/docs/gdk4/enum.SurfaceEdge.html)
- [Original viewport2 design doc](docs/design/2026-03-18-viewport2.md)
