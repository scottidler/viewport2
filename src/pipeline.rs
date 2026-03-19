use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::capture::Frame;
use crate::convert;
use crate::output;
use crate::rect::AtomicRect;

pub struct PipelineConfig {
    pub device: String,
    pub output_width: u32,
    pub output_height: u32,
    pub target_fps: u32,
}

/// Run the frame processing pipeline: crop -> resize -> convert -> write.
/// Blocks until the frame channel is closed (sender dropped).
pub fn run(config: PipelineConfig, frame_rx: mpsc::Receiver<Frame>, shared_rect: Arc<AtomicRect>) {
    let mut v4l2 = match output::V4l2Output::open(&config.device, config.output_width, config.output_height) {
        Ok(v) => v,
        Err(e) => {
            log::error!("Failed to open v4l2loopback: {}", e);
            return;
        }
    };

    let mut converter = convert::Converter::new(config.output_width, config.output_height);
    let mut resize_buf = vec![0u8; (config.output_width * config.output_height * 4) as usize];
    let mut yuyv_buf = vec![0u8; (config.output_width * config.output_height * 2) as usize];
    let mut count = 0u64;
    let frame_interval = Duration::from_nanos(1_000_000_000 / config.target_fps as u64);
    let mut next_frame_time = Instant::now();

    while let Ok(frame) = frame_rx.recv() {
        // FPS throttle: skip frames that arrive faster than target fps
        let now = Instant::now();
        if now < next_frame_time {
            continue;
        }
        next_frame_time = now + frame_interval;

        // Crop to overlay rect
        let crop_rect = shared_rect.get();
        let (cropped, crop_w, crop_h) =
            convert::crop_bgrx(&frame.data, frame.width, frame.height, frame.stride, &crop_rect);

        // Resize cropped region to output resolution
        convert::resize_bgrx_nearest(
            &cropped,
            crop_w,
            crop_h,
            crop_w * 4,
            &mut resize_buf,
            config.output_width,
            config.output_height,
        );

        // Convert BGRx -> YUYV (SIMD-accelerated via yuvutils-rs)
        converter.bgrx_to_yuyv(&resize_buf, config.output_width * 4, &mut yuyv_buf);

        // Write to v4l2loopback device
        if let Err(e) = v4l2.write_frame(&yuyv_buf) {
            log::error!("Failed to write frame: {}", e);
            break;
        }

        count += 1;
        if count.is_multiple_of(30) {
            log::info!(
                "Written {} frames (crop {}x{} at {},{} -> {}x{})",
                count,
                crop_rect.width,
                crop_rect.height,
                crop_rect.x,
                crop_rect.y,
                config.output_width,
                config.output_height,
            );
        }
    }
}
