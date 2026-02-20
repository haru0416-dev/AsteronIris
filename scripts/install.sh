#!/usr/bin/env bash
set -euo pipefail

# AsteronIris Installer
# Usage: curl -fsSL https://raw.githubusercontent.com/haru0416-dev/AsteronIris/main/scripts/install.sh | bash

REPO="haru0416-dev/AsteronIris"
BINARY_NAME="asteroniris"
INSTALL_DIR="${ASTERONIRIS_INSTALL_DIR:-/usr/local/bin}"

# ── Color output ──
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

info()  { printf "${CYAN}info${NC}  %s\n" "$1"; }
ok()    { printf "${GREEN}  ✓${NC}  %s\n" "$1"; }
warn()  { printf "${YELLOW}warn${NC}  %s\n" "$1"; }
error() { printf "${RED}error${NC} %s\n" "$1" >&2; }
fail()  { error "$1"; exit 1; }

# ── Dependency check ──
require_cmd() {
    command -v "$1" >/dev/null 2>&1 || fail "Required command not found: $1"
}

require_cmd curl
require_cmd tar

# ── Detect platform ──
detect_platform() {
    local os arch

    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux)  os="unknown-linux-gnu" ;;
        Darwin) os="apple-darwin" ;;
        *)      fail "Unsupported OS: $os (Linux and macOS only)" ;;
    esac

    case "$arch" in
        x86_64|amd64)   arch="x86_64" ;;
        aarch64|arm64)  arch="aarch64" ;;
        *)              fail "Unsupported architecture: $arch" ;;
    esac

    # aarch64-linux not yet in release matrix
    if [[ "$arch" == "aarch64" && "$os" == "unknown-linux-gnu" ]]; then
        fail "aarch64-linux builds are not yet available. Please build from source: cargo build --release"
    fi

    echo "${arch}-${os}"
}

# ── Get latest release tag ──
get_latest_version() {
    local url="https://api.github.com/repos/${REPO}/releases/latest"
    local version

    version=$(curl -fsSL "$url" | grep '"tag_name"' | head -1 | sed -E 's/.*"tag_name":\s*"([^"]+)".*/\1/')

    if [[ -z "$version" ]]; then
        fail "Could not determine latest version from GitHub"
    fi

    echo "$version"
}

# ── Download and install ──
install() {
    local platform version archive_name url tmpdir

    info "Detecting platform..."
    platform="$(detect_platform)"
    ok "Platform: ${platform}"

    info "Fetching latest release..."
    version="$(get_latest_version)"
    ok "Version: ${version}"

    archive_name="${BINARY_NAME}-${platform}.tar.gz"
    url="https://github.com/${REPO}/releases/download/${version}/${archive_name}"

    info "Downloading ${archive_name}..."
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT

    if ! curl -fsSL "$url" -o "${tmpdir}/${archive_name}"; then
        fail "Download failed. Check: ${url}"
    fi
    ok "Downloaded"

    info "Extracting..."
    tar xzf "${tmpdir}/${archive_name}" -C "$tmpdir"

    if [[ ! -f "${tmpdir}/${BINARY_NAME}" ]]; then
        fail "Binary not found in archive"
    fi

    info "Installing to ${INSTALL_DIR}/${BINARY_NAME}..."
    if [[ -w "$INSTALL_DIR" ]]; then
        mv "${tmpdir}/${BINARY_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"
    else
        warn "Need sudo to write to ${INSTALL_DIR}"
        sudo mv "${tmpdir}/${BINARY_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"
    fi
    chmod +x "${INSTALL_DIR}/${BINARY_NAME}"
    ok "Installed: ${INSTALL_DIR}/${BINARY_NAME}"

    echo ""
    info "AsteronIris ${version} installed successfully!"
    echo ""
    echo "  Get started:"
    echo "    ${BINARY_NAME} onboard        # Set up workspace and config"
    echo "    ${BINARY_NAME} agent           # Start the AI assistant"
    echo "    ${BINARY_NAME} --help          # See all commands"
    echo ""
    echo "  Documentation:"
    echo "    https://github.com/${REPO}"
    echo ""
}

install
