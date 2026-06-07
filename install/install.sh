#!/usr/bin/env bash
# kioskwm installer — Ubuntu/Debian (x86_64)
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/mdf-leon/kioskwm/main/install/install.sh | bash
#   curl -fsSL ... | bash -s -- --version v0.1.0
set -euo pipefail

REPO="${KIOSKWM_REPO:-mdf-leon/kioskwm}"
BIN_NAME="kioskwm"
INSTALL_DIR="${KIOSKWM_INSTALL_DIR:-/usr/local/bin}"
INSTALL_BIN="${INSTALL_DIR}/${BIN_NAME}"
RELEASE="${KIOSKWM_VERSION:-latest}"
ASSET="kioskwm-x86_64-unknown-linux-gnu"
BASE_URL="https://github.com/${REPO}/releases"

log() { printf '==> %s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

need_cmd() {
    command -v "$1" >/dev/null 2>&1 || die "comando obrigatório ausente: $1"
}

as_root() {
    if [ "$(id -u)" -eq 0 ]; then
        "$@"
    elif command -v sudo >/dev/null 2>&1; then
        sudo "$@"
    else
        die "precisa de root (rode com sudo ou como root)"
    fi
}

parse_args() {
    while [ $# -gt 0 ]; do
        case "$1" in
            --version)
                RELEASE="$2"
                shift 2
                ;;
            -h | --help)
                cat <<EOF
Instala ${BIN_NAME} em ${INSTALL_DIR}

Uso:
  curl -fsSL https://raw.githubusercontent.com/${REPO}/main/install/install.sh | bash
  curl -fsSL ... | bash -s -- --version v0.1.0

Variáveis:
  KIOSKWM_VERSION=v0.1.0   versão do release (padrão: latest)
  KIOSKWM_INSTALL_DIR=/usr/local/bin
EOF
                exit 0
                ;;
            *)
                die "argumento desconhecido: $1 (use --help)"
                ;;
        esac
    done
}

detect_os() {
    [ -f /etc/os-release ] || die "sistema não suportado (sem /etc/os-release)"
    # shellcheck disable=SC1091
    . /etc/os-release
    case "${ID:-}" in
        ubuntu | debian | pop | linuxmint)
            log "detectado: ${PRETTY_NAME:-$ID}"
            ;;
        *)
            die "distro não suportada ainda: ${ID:-unknown} (use Ubuntu/Debian x86_64)"
            ;;
    esac
}

detect_arch() {
    local arch
    arch="$(uname -m)"
    [ "$arch" = "x86_64" ] || die "arquitetura não suportada: $arch (só x86_64 por enquanto)"
}

install_runtime_deps() {
    log "instalando dependências de runtime..."
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
    [ -n "$tag" ] || die "nenhum release encontrado em github.com/${REPO}"
    printf '%s' "$tag"
}

download_and_verify() {
    local tag="$1"
    local tmpdir
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "${tmpdir:-}"' EXIT

    resolve_download_urls "$tag"
    log "baixando ${tag}..."
    curl -fsSL -o "${tmpdir}/${ASSET}" "$DOWNLOAD_URL"
    curl -fsSL -o "${tmpdir}/${ASSET}.sha256" "$CHECKSUM_URL"

    (
        cd "$tmpdir"
        need_cmd sha256sum
        sha256sum -c "${ASSET}.sha256"
    )

    as_root install -d "$INSTALL_DIR"
    as_root install -m 755 "${tmpdir}/${ASSET}" "$INSTALL_BIN"
    log "instalado em ${INSTALL_BIN}"
}

verify_install() {
    command -v "$BIN_NAME" >/dev/null 2>&1 || die "instalação falhou: ${BIN_NAME} não está no PATH"
    log "versão instalada:"
    "$BIN_NAME" --version
    log "pronto — rode: kioskwm --desktop   (aninhado) ou kioskwm   (TTY/kiosk)"
}

main() {
    parse_args "$@"
    detect_os
    detect_arch
    need_cmd curl
    command -v python3 >/dev/null 2>&1 || die "python3 é necessário para resolver releases"

    local tag="$RELEASE"
    if [ "$tag" = "latest" ]; then
        tag="$(resolve_latest_tag)"
    fi

    install_runtime_deps
    download_and_verify "$tag"
    verify_install
}

main "$@"
