use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "viewport",
    about = "Draggable screen region capture tool that outputs to a virtual camera",
    version = env!("GIT_DESCRIBE"),
    after_help = "Logs are written to: ~/.local/share/viewport/logs/viewport.log"
)]
pub struct Cli {
    /// Path to config file
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    /// v4l2loopback device path
    #[arg(short, long)]
    pub device: Option<String>,

    /// Initial overlay size (WxH)
    #[arg(short, long)]
    pub size: Option<String>,

    /// Target frame rate
    #[arg(short, long)]
    pub fps: Option<u32>,

    /// Border color as hex (e.g., #ff3333)
    #[arg(long)]
    pub color: Option<String>,

    /// Border width in pixels
    #[arg(long)]
    pub border_width: Option<u32>,

    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long)]
    pub log_level: Option<String>,
}
