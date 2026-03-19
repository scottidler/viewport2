# viewport2

Draggable screen region capture tool that outputs to a virtual camera for presentations.

Share a laptop-sized portion of your 4K desktop in Google Meet or Slack without overwhelming the audience. Position a resizable overlay frame on your screen, and viewport2 outputs everything inside that frame as a virtual camera feed via v4l2loopback.

## Prerequisites

- Linux with Wayland (GNOME/Mutter recommended)
- PipeWire (running)
- XDG Desktop Portal (comes with GNOME)
- v4l2loopback kernel module

### System packages

```bash
# Build dependencies
sudo apt install libgtk-4-dev libpipewire-0.3-dev

# Runtime: v4l2loopback
sudo apt install v4l2loopback-dkms
```

## Install

```bash
cargo install --path .
```

## Setup

Load the v4l2loopback kernel module:

```bash
sudo modprobe v4l2loopback devices=1 video_nr=10 card_label="Viewport" exclusive_caps=1
```

To load automatically on boot, add to `/etc/modules-load.d/v4l2loopback.conf`:

```
v4l2loopback
```

And configure options in `/etc/modprobe.d/v4l2loopback.conf`:

```
options v4l2loopback devices=1 video_nr=10 card_label="Viewport" exclusive_caps=1
```

## Quick Start

```bash
# 1. Load the virtual camera kernel module (one-time, or add to boot config above)
sudo modprobe v4l2loopback devices=1 video_nr=10 card_label="Viewport" exclusive_caps=1

# 2. Start viewport2
viewport2

# 3. A portal dialog appears asking for screen capture permission - approve it
#    (This is remembered for future runs)

# 4. A red-bordered overlay frame appears on your desktop

# 5. Right-click the viewport2 window in the GNOME top bar and select
#    "Always on Top" so the frame stays visible over other windows

# 6. Open Google Meet (or Slack, Zoom, etc.)
#    Go to Settings > Video > Camera and select "Viewport"

# 7. Position and resize the overlay frame over the area you want to share
#    - Drag the interior to move the window
#    - Drag edges/corners to resize
#    - Use arrow keys to fine-tune crop position
#    - Press 1/2/3 for preset sizes

# 8. Your meeting participants now see exactly what's inside the red frame

# 9. Press Escape to quit when done
```

## Usage

```bash
viewport2
```

The workflow is: viewport2 captures your full screen via PipeWire, crops to the overlay frame's position and size, converts to YUYV, and writes to a v4l2loopback virtual camera device. Any app that can select a camera (Meet, Slack, Zoom, OBS) will see "Viewport" as an available camera source showing exactly what's inside the red frame.

### CLI options

```
viewport2 [OPTIONS]

Options:
  -c, --config <PATH>         Path to config file
  -d, --device <PATH>         v4l2loopback device path [default: /dev/video10]
  -s, --size <WxH>            Initial overlay size [default: 1280x720]
  -f, --fps <N>               Target frame rate [default: 30]
  -l, --log-level <LEVEL>     Log level: trace, debug, info, warn, error [default: info]
      --color <HEX>           Border color [default: #ff3333]
      --border-width <PX>     Border width [default: 4]
```

You can also set the log level via the `VIEWPORT2_LOG` environment variable. Logs are written to `~/.local/share/viewport2/logs/` with daily rotation.

### Keyboard shortcuts

| Key | Action |
|-----|--------|
| `Escape` | Quit |
| `Arrow keys` | Nudge crop position by 10px |
| `Shift+Arrow` | Nudge crop position by 1px |
| `Ctrl+Arrow` | Resize by 10px |
| `Ctrl+Shift+Arrow` | Resize by 1px |
| `1` | Preset: 1280x720 |
| `2` | Preset: 1920x1080 |
| `3` | Preset: 960x540 |

### Config file

Place at `~/.config/viewport2/viewport2.yml`:

```yaml
device: /dev/video10
output_size:
  width: 1280
  height: 720
initial_size:
  width: 1280
  height: 720
fps: 30
border_color: "#ff3333"
border_width: 4
presets:
  - width: 1280
    height: 720
  - width: 1920
    height: 1080
  - width: 960
    height: 540
```

CLI arguments override config file values.

## Troubleshooting

**Device not found:** Load v4l2loopback with `sudo modprobe v4l2loopback devices=1 video_nr=10 card_label="Viewport" exclusive_caps=1`

**Permission denied on device:** Add your user to the video group: `sudo usermod -aG video $USER` (then log out and back in)

**Portal dialog not appearing:** Ensure `xdg-desktop-portal` and `xdg-desktop-portal-gnome` are installed and running.

**Camera not showing in Meet/Slack:** Make sure `exclusive_caps=1` was set when loading v4l2loopback. Some apps only detect devices that report exclusive capabilities.

## License

MIT
