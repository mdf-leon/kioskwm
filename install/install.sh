#!/usr/bin/env bash
# kioskwm installer — Ubuntu/Debian (x86_64)
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/mdf-leon/kioskwm/main/install/install.sh | bash
#   curl -fsSL ... | bash -s -- --from-source
#   curl -fsSL ... | bash -s -- --version v0.1.0
set -euo pipefail

REPO="${KIOSKWM_REPO:-mdf-leon/kioskwm}"
BIN_NAME="kioskwm"
INSTALL_DIR="${KIOSKWM_INSTALL_DIR:-/usr/local/bin}"
INSTALL_BIN="${INSTALL_DIR}/${BIN_NAME}"
RELEASE="${KIOSKWM_VERSION:-latest}"
ASSET="kioskwm-x86_64-unknown-linux-gnu"
BASE_URL="https://github.com/${REPO}/releases"
FROM_SOURCE="${KIOSKWM_FROM_SOURCE:-0}"

log() { printf '==> %s\n' "$*"; }
warn() { printf '!! %s\n' "$*" >&2; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

need_cmd() {
    command -v "$1" >/dev/null 2>&1 || die "missing command: $1"
}

as_root() {
    if [ "$(id -u)" -eq 0 ]; then
        "$@"
    elif command -v sudo >/dev/null 2>&1; then
        sudo "$@"
    else
        die "need root (run with sudo or as root)"
    fi
}

parse_args() {
    while [ $# -gt 0 ]; do
        case "$1" in
            --from-source)
                FROM_SOURCE=1
                shift
                ;;
            --version)
                RELEASE="$2"
                shift 2
                ;;
            -h | --help)
                cat <<EOF
Install ${BIN_NAME} to ${INSTALL_DIR}

Usage:
  curl -fsSL https://raw.githubusercontent.com/${REPO}/main/install/install.sh | bash
  curl -fsSL ... | bash -s -- --from-source
  curl -fsSL ... | bash -s -- --version v0.1.0

Options:
  --from-source     Build from GitHub source (recommended on Ubuntu 22.04 VMs)

Environment:
  KIOSKWM_VERSION=v0.1.0
  KIOSKWM_FROM_SOURCE=1
  KIOSKWM_INSTALL_DIR=/usr/local/bin
  KIOSKWM_SRC=/path/to/repo   (optional, for --from-source)
EOF
                exit 0
                ;;
            *)
                die "unknown argument: $1 (use --help)"
                ;;
        esac
    done
}

detect_os() {
    [ -f /etc/os-release ] || die "unsupported system (no /etc/os-release)"
    # shellcheck disable=SC1091
    . /etc/os-release
    case "${ID:-}" in
        ubuntu | debian | pop | linuxmint)
            log "detected: ${PRETTY_NAME:-$ID}"
            ;;
        *)
            die "unsupported distro: ${ID:-unknown} (Ubuntu/Debian x86_64 only for now)"
            ;;
    esac
}

detect_arch() {
    local arch
    arch="$(uname -m)"
    [ "$arch" = "x86_64" ] || die "unsupported architecture: $arch (x86_64 only)"
}

install_runtime_deps() {
    log "installing runtime dependencies..."
    as_root apt-get update -qq
    local pkgs=(
        ca-certificates
        curl
        libseat1
        libinput10
        libgbm1
        libxkbcommon0
        libudev1
        libdrm2
        libwacom9
        libevdev2
        xwayland
    )
    for mtdev in libmtdev1t64 libmtdev1; do
        if apt-cache show "$mtdev" >/dev/null 2>&1; then
            pkgs+=("$mtdev")
            break
        fi
    done
    as_root DEBIAN_FRONTEND=noninteractive apt-get install -y -qq "${pkgs[@]}"
}

install_build_deps() {
    log "installing build dependencies..."
    as_root apt-get update -qq
    as_root DEBIAN_FRONTEND=noninteractive apt-get install -y -qq \
        build-essential pkg-config curl git \
        libseat-dev libinput-dev libudev-dev libxkbcommon-dev \
        libgbm-dev libdrm-dev libegl1-mesa-dev libwayland-dev libsystemd-dev \
        libpixman-1-dev
}

ensure_rust() {
    if command -v cargo >/dev/null 2>&1; then
        return
    fi
    need_cmd curl
    log "installing Rust (rustup)..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    # shellcheck disable=SC1091
    source "${HOME}/.cargo/env"
    need_cmd cargo
}

build_from_source() {
    install_build_deps
    ensure_rust
    # shellcheck disable=SC1091
    [ -f "${HOME}/.cargo/env" ] && source "${HOME}/.cargo/env"

    local src="${KIOSKWM_SRC:-}"
    local build_dir
    if [ -n "$src" ] && [ -f "$src/Cargo.toml" ]; then
        build_dir="$src"
        log "building from KIOSKWM_SRC=$build_dir"
    else
        build_dir="$(mktemp -d)"
        trap 'rm -rf "${build_dir:-}"' RETURN
        log "cloning https://github.com/${REPO}.git ..."
        need_cmd git
        git clone --depth 1 "https://github.com/${REPO}.git" "$build_dir"
    fi

    log "cargo build --release (this may take several minutes)..."
    (cd "$build_dir" && cargo build --release)

    as_root install -d "$INSTALL_DIR"
    as_root install -m 755 "$build_dir/target/release/kioskwm" "$INSTALL_BIN"
    log "installed (built from source) at ${INSTALL_BIN}"
}

resolve_download_urls() {
    local tag="$1"
    DOWNLOAD_URL="${BASE_URL}/download/${tag}/${ASSET}"
    CHECKSUM_URL="${BASE_URL}/download/${tag}/${ASSET}.sha256"
}

resolve_latest_tag() {
    need_cmd curl
    local api="https://api.github.com/repos/${REPO}/releases/latest"
    local tag
    tag="$(
        curl -fsSL "$api" | python3 -c 'import sys, json; print(json.load(sys.stdin)["tag_name"])'
    )"
    [ -n "$tag" ] || die "no release found at github.com/${REPO}"
    printf '%s' "$tag"
}

download_and_verify() {
    local tag="$1"
    local tmpdir
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "${tmpdir:-}"' EXIT

    resolve_download_urls "$tag"
    log "downloading ${tag}..."
    curl -fsSL -o "${tmpdir}/${ASSET}" "$DOWNLOAD_URL"
    curl -fsSL -o "${tmpdir}/${ASSET}.sha256" "$CHECKSUM_URL"

    (
        cd "$tmpdir"
        need_cmd sha256sum
        local expected actual
        expected="$(awk '{print $1}' "${ASSET}.sha256")"
        [ -n "$expected" ] || die "invalid checksum file"
        actual="$(sha256sum "${ASSET}" | awk '{print $1}')"
        [ "$expected" = "$actual" ] || die "checksum mismatch for ${ASSET}"
    )

    as_root install -d "$INSTALL_DIR"
    as_root install -m 755 "${tmpdir}/${ASSET}" "$INSTALL_BIN"
    log "installed release binary at ${INSTALL_BIN}"
}

binary_runs() {
    "$INSTALL_BIN" --version >/dev/null 2>&1
}

verify_install() {
    command -v "$BIN_NAME" >/dev/null 2>&1 || die "install failed: ${BIN_NAME} not in PATH"
    if binary_runs; then
        log "installed version:"
        "$BIN_NAME" --version
        log "done — run: kioskwm --desktop  (nested)  or  kioskwm  (TTY/kiosk)"
        return 0
    fi

    local err
    err="$("$INSTALL_BIN" --version 2>&1)" || true
    if [[ "$err" == *GLIBC_* ]]; then
        warn "release binary incompatible with this system glibc:"
        warn "  $err"
        return 1
    fi
    die "failed to run ${BIN_NAME}: $err"
}

main() {
    parse_args "$@"
    detect_os
    detect_arch
    need_cmd curl
    command -v python3 >/dev/null 2>&1 || die "python3 is required to resolve GitHub releases"

    install_runtime_deps

    if [ "$FROM_SOURCE" = "1" ]; then
        build_from_source
        verify_install || die "build from source failed"
        return
    fi

    local tag="$RELEASE"
    if [ "$tag" = "latest" ]; then
        tag="$(resolve_latest_tag)"
    fi

    download_and_verify "$tag"
    if verify_install; then
        return
    fi

    warn "no compatible prebuilt binary for this system — compiling from source (~5–15 min, installs Rust once)"
    build_from_source
    verify_install || die "build from source failed"
}

main "$@"
