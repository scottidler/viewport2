use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;

use eyre::{Context, Result};
use tracing::instrument;

// V4L2 constants
const V4L2_BUF_TYPE_VIDEO_OUTPUT: u32 = 2;
const V4L2_FIELD_NONE: u32 = 1;
const V4L2_COLORSPACE_SRGB: u32 = 8;

fn v4l2_fourcc(a: u8, b: u8, c: u8, d: u8) -> u32 {
    (a as u32) | ((b as u32) << 8) | ((c as u32) << 16) | ((d as u32) << 24)
}

#[repr(C)]
#[derive(Clone, Copy)]
struct V4l2PixFormat {
    width: u32,
    height: u32,
    pixelformat: u32,
    field: u32,
    bytesperline: u32,
    sizeimage: u32,
    colorspace: u32,
    priv_: u32,
    flags: u32,
    encoding: u32,
    quantization: u32,
    xfer_func: u32,
}

/// Matches the kernel's `struct v4l2_format`.
/// The `fmt` union is 200 bytes; we use `pix` (48 bytes) + 152 bytes padding.
#[repr(C)]
struct V4l2Format {
    type_: u32,
    pix: V4l2PixFormat,
    _padding: [u8; 152],
}

nix::ioctl_readwrite!(vidioc_s_fmt, b'V', 5, V4l2Format);

pub struct V4l2Output {
    file: File,
    frame_size: usize,
}

impl V4l2Output {
    /// Open a v4l2loopback device and set YUYV format at the given resolution.
    #[instrument(skip_all, fields(device, width, height))]
    pub fn open(device: &str, width: u32, height: u32) -> Result<Self> {
        let file = OpenOptions::new().write(true).open(device).context(format!(
            "Failed to open v4l2loopback device '{}' - is the module loaded?",
            device
        ))?;

        let bytesperline = width * 2; // YUYV: 2 bytes per pixel
        let sizeimage = bytesperline * height;

        let mut fmt = V4l2Format {
            type_: V4L2_BUF_TYPE_VIDEO_OUTPUT,
            pix: V4l2PixFormat {
                width,
                height,
                pixelformat: v4l2_fourcc(b'Y', b'U', b'Y', b'V'),
                field: V4L2_FIELD_NONE,
                bytesperline,
                sizeimage,
                colorspace: V4L2_COLORSPACE_SRGB,
                priv_: 0,
                flags: 0,
                encoding: 0,
                quantization: 0,
                xfer_func: 0,
            },
            _padding: [0u8; 152],
        };

        // Safety: V4l2Format matches the kernel ABI, and the fd is a valid v4l2 device.
        unsafe { vidioc_s_fmt(file.as_raw_fd(), &mut fmt) }
            .context("VIDIOC_S_FMT failed - is v4l2loopback loaded and is the device path correct?")?;

        tracing::info!(
            "v4l2loopback device '{}' configured: {}x{} YUYV ({} bytes/frame)",
            device,
            width,
            height,
            sizeimage
        );

        Ok(Self {
            file,
            frame_size: sizeimage as usize,
        })
    }

    /// Write a YUYV frame to the device.
    pub fn write_frame(&mut self, yuyv_data: &[u8]) -> Result<()> {
        if yuyv_data.len() != self.frame_size {
            eyre::bail!(
                "Frame size mismatch: expected {} bytes, got {}",
                self.frame_size,
                yuyv_data.len()
            );
        }
        self.file
            .write_all(yuyv_data)
            .context("Failed to write frame to v4l2loopback")?;
        Ok(())
    }
}
