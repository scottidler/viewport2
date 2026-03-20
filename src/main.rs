#![deny(clippy::unwrap_used)]
#![deny(dead_code)]
#![deny(unused_variables)]

use std::fs;
use std::sync::mpsc;

use clap::Parser;
use eyre::{Context, Result};
use tracing::instrument;

mod capture;
mod cli;
mod config;
mod convert;
mod logging;
mod output;
mod overlay;
mod pipeline;
mod rect;

use cli::Cli;
use config::Config;
use rect::AtomicRect;

#[instrument(skip(config))]
fn preflight_checks(config: &Config) -> Result<()> {
    let device = std::path::Path::new(&config.device);
    if !device.exists() {
        eyre::bail!(
            "v4l2loopback device '{}' not found.\n\
             Load the kernel module with:\n  \
             sudo modprobe v4l2loopback devices=1 video_nr={} card_label=\"Viewport\" exclusive_caps=1",
            config.device,
            config.device.strip_prefix("/dev/video").unwrap_or("10")
        );
    }

    if let Err(e) = fs::OpenOptions::new().write(true).open(device) {
        eyre::bail!(
            "Cannot open '{}' for writing: {}\n\
             Ensure your user is in the 'video' group:\n  \
             sudo usermod -aG video $USER\n  \
             (then log out and back in)",
            config.device,
            e
        );
    }

    tracing::debug!(device = %config.device, "preflight checks passed");
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let level = logging::resolve_log_level(cli.log_level.as_deref());
    #[allow(unused_variables)]
    let guard = logging::setup_tracing(&level).context("Failed to setup tracing")?;

    let mut config = Config::load(&cli).context("Failed to load configuration")?;

    tracing::info!(
        version = env!("GIT_DESCRIBE"),
        device = %config.device,
        output_size = %config.output_size,
        fps = config.fps,
        "viewport starting"
    );

    preflight_checks(&config)?;

    // First-run hint: always-on-top requires manual user action on Wayland
    if config.portal_restore_token.is_none() {
        eprintln!(
            "Tip: Right-click the viewport window in the GNOME top bar and select \
             'Always on Top' to keep the overlay visible."
        );
    }

    // Negotiate screen capture via XDG portal (async)
    let session = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("Failed to create tokio runtime")?
        .block_on(capture::create_session(&config))
        .context(
            "Screen capture denied or unavailable.\n\
             Ensure XDG Desktop Portal is running (comes with GNOME).\n\
             If the portal dialog was dismissed, try again - the permission prompt is required on first use.",
        )?;

    // Persist the restore token so the user isn't prompted next time
    if let Some(token) = &session.restore_token {
        config.portal_restore_token = Some(token.clone());
        if let Err(e) = config.save() {
            tracing::warn!(error = %e, "failed to save portal restore token");
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
                tracing::error!(error = %e, "PipeWire capture failed");
            }
        })
        .context("Failed to spawn PipeWire capture thread")?;

    // v4l2loopback output thread: crop -> resize -> BGRx-to-YUYV -> write
    let pipeline_config = pipeline::PipelineConfig {
        device: config.device.clone(),
        output_width: config.output_size.width,
        output_height: config.output_size.height,
        target_fps: config.fps,
    };
    let output_rect = shared_rect.clone();
    let output_handle = std::thread::Builder::new()
        .name("v4l2-output".into())
        .spawn(move || pipeline::run(pipeline_config, frame_rx, output_rect))
        .context("Failed to spawn pipeline thread")?;

    // Run overlay on main thread (GTK4 requires main thread)
    overlay::run(&config, shared_rect)?;

    // Overlay closed - clean up
    drop(output_handle);
    drop(pw_handle);

    tracing::info!("viewport shutting down");
    Ok(())
}
