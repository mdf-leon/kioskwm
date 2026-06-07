# kioskwm

Wayland compositor for kiosk deployments and nested desktop testing.

## Install (Ubuntu/Debian x86_64)

```bash
curl -fsSL https://raw.githubusercontent.com/mdf-leon/kioskwm/main/install/install.sh | bash
```

Then run:

```bash
kioskwm --desktop    # nested inside Konsole/Plasma
kioskwm              # TTY kiosk (DRM + libseat)
```

Pin a version:

```bash
curl -fsSL https://raw.githubusercontent.com/mdf-leon/kioskwm/main/install/install.sh | bash -s -- --version v0.1.0
```

Site: https://mdf-leon.github.io/kioskwm/

## Features

- Fullscreen kiosk mode on TTY (DRM + libseat)
- Nested mode inside Plasma/KDE (`--desktop`)
- XWayland support
- WM menu (Ctrl+Alt+Del), context menu, Alt+Tab
- Settings panel with mouse speed and power actions

## Quick start (dev)

```bash
cargo run --release -- --desktop
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
