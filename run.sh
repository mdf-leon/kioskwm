#!/usr/bin/env bash
# Detecta o ambiente automaticamente:
#   Konsole/Alacritty → janela aninhada no desktop
#   tty2/tty3/...     → compositor fullscreen via DRM
set -euo pipefail
cd "$(dirname "$0")"

export RUST_LOG="${RUST_LOG:-kioskwm=info}"
export KIOSKWM_LOG="${KIOSKWM_LOG:-/tmp/kioskwm.log}"

exec cargo run --release -- "$@"
