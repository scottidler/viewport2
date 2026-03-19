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

## Usage

```bash
viewport2
```

On first run, a portal dialog asks for screen capture permission. This is remembered for subsequent runs.

A red-bordered overlay frame appears on your desktop. In Google Meet or Slack, select "Viewport" as your camera source.

**Always-on-top:** Right-click the viewport2 window title in the GNOME top bar (or Super+right-click the window) and select "Always on Top." This keeps the overlay visible above other windows. GTK4 on Wayland does not support programmatic always-on-top, so this is a one-time manual step.

### CLI options

```
viewport2 [OPTIONS]

Options:
  -c, --config <PATH>         Path to config file
  -d, --device <PATH>         v4l2loopback device path [default: /dev/video10]
  -s, --size <WxH>            Initial overlay size [default: 1280x720]
  -f, --fps <N>               Target frame rate [default: 30]
      --color <HEX>           Border color [default: #ff3333]
      --border-width <PX>     Border width [default: 4]
  -v, --verbose               Enable verbose output
```

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
