#![deny(clippy::unwrap_used)]
#![deny(dead_code)]
#![deny(unused_variables)]

use std::sync::mpsc;

use clap::Parser;
use eyre::{Context, Result};
use log::info;
use std::fs;
use std::path::PathBuf;

mod capture;
mod cli;
mod config;
mod convert;
mod output;
mod overlay;
mod rect;

use cli::Cli;
use config::Config;
use rect::AtomicRect;

fn setup_logging() -> Result<()> {
    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("viewport2")
        .join("logs");

    fs::create_dir_all(&log_dir).context("Failed to create log directory")?;

    let log_file = log_dir.join("viewport2.log");

    let target = Box::new(
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file)
            .context("Failed to open log file")?,
    );

    env_logger::Builder::from_default_env()
        .target(env_logger::Target::Pipe(target))
        .init();

    info!("Logging initialized, writing to: {}", log_file.display());
    Ok(())
}

fn main() -> Result<()> {
    setup_logging().context("Failed to setup logging")?;

    let cli = Cli::parse();
    let mut config = Config::load(&cli).context("Failed to load configuration")?;

    info!("Starting viewport2 with config: {:?}", config);

    // Negotiate screen capture via XDG portal (async)
    let session = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("Failed to create tokio runtime")?
        .block_on(capture::create_session(&config))
        .context("Failed to create capture session")?;

    // Persist the restore token so the user isn't prompted next time
    if let Some(token) = &session.restore_token {
        config.portal_restore_token = Some(token.clone());
        if let Err(e) = config.save() {
            log::warn!("Failed to save portal restore token: {}", e);
        }
    }

    let shared_rect = AtomicRect::new(0, 0, config.initial_size.width, config.initial_size.height);

    // Channel for frames from PipeWire -> output pipeline
    let (frame_tx, frame_rx) = mpsc::sync_channel::<capture::Frame>(2);

    // Spawn PipeWire capture on a background thread
    let pw_handle = std::thread::Builder::new()
        .name("pipewire-capture".into())
        .spawn(move || {
            if let Err(e) = capture::run_pipewire_stream(session, frame_tx) {
                log::error!("PipeWire capture failed: {}", e);
            }
        })
        .context("Failed to spawn PipeWire capture thread")?;

    // v4l2loopback output thread: resize -> BGRx-to-YUYV -> write
    let output_width = config.output_size.width;
    let output_height = config.output_size.height;
    let device = config.device.clone();
    let output_handle = std::thread::Builder::new()
        .name("v4l2-output".into())
        .spawn(move || {
            let mut v4l2 = match output::V4l2Output::open(&device, output_width, output_height) {
                Ok(v) => v,
                Err(e) => {
                    log::error!("Failed to open v4l2loopback: {}", e);
                    return;
                }
            };

            let mut resize_buf = vec![0u8; (output_width * output_height * 4) as usize];
            let mut yuyv_buf = vec![0u8; (output_width * output_height * 2) as usize];
            let mut count = 0u64;

            while let Ok(frame) = frame_rx.recv() {
                // Resize source frame to output resolution (nearest-neighbor)
                convert::resize_bgrx_nearest(
                    &frame.data,
                    frame.width,
                    frame.height,
                    frame.stride,
                    &mut resize_buf,
                    output_width,
                    output_height,
                );

                // Convert BGRx -> YUYV
                convert::bgrx_to_yuyv(
                    &resize_buf,
                    output_width,
                    output_height,
                    output_width * 4,
                    &mut yuyv_buf,
                );

                // Write to v4l2loopback device
                if let Err(e) = v4l2.write_frame(&yuyv_buf) {
                    log::error!("Failed to write frame: {}", e);
                    break;
                }

                count += 1;
                if count.is_multiple_of(30) {
                    log::info!(
                        "Written {} frames to v4l2loopback ({}x{} -> {}x{})",
                        count,
                        frame.width,
                        frame.height,
                        output_width,
                        output_height,
                    );
                }
            }
        })
        .context("Failed to spawn v4l2 output thread")?;

    // Run overlay on main thread (GTK4 requires main thread)
    overlay::run(&config, shared_rect)?;

    // Overlay closed - clean up
    drop(output_handle);
    drop(pw_handle);

    Ok(())
}
