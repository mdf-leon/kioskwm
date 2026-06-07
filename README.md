# kioskwm

Wayland compositor for kiosk deployments and nested desktop testing.

## Features

- Fullscreen kiosk mode on TTY (DRM + libseat)
- Nested mode inside Plasma/KDE (`--desktop`)
- XWayland support
- WM menu (Ctrl+Alt+Del), context menu, Alt+Tab
- Settings panel with mouse speed and power actions

## Quick start (dev)

```bash
# nested desktop (Konsole/Alacritty)
cargo run --release -- --desktop

# TTY kiosk
cargo run --release
```

Runtime deps on Ubuntu:

```bash
sudo apt install libseat1 libinput10 libgbm1 libxkbcommon0 libudev1 libdrm2 xwayland
```

Build deps:

```bash
sudo apt install build-essential pkg-config libseat-dev libinput-dev libudev-dev libxkbcommon-dev libgbm-dev
```

## License

MIT
