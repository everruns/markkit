#!/usr/bin/env bash
# Initialize cloud agent environment
# Installs: just, gh, doppler

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { echo -e "${GREEN}[info]${NC} $*"; }
warn()  { echo -e "${YELLOW}[warn]${NC} $*"; }
error() { echo -e "${RED}[error]${NC} $*" >&2; }

ARCH=$(uname -m)
case "$ARCH" in
    x86_64)  ARCH_LABEL="amd64" ;;
    aarch64) ARCH_LABEL="arm64" ;;
    *) error "Unsupported architecture: $ARCH"; exit 1 ;;
esac

INSTALL_DIR="/usr/local/bin"

# ─── Install just ────────────────────────────────────────────

install_just() {
    info "Installing just..."
    curl --retry 3 --connect-timeout 10 -sSfL https://just.systems/install.sh | bash -s -- --to "$INSTALL_DIR"
    info "just installed: $(just --version)"
}

# ─── Install gh ──────────────────────────────────────────────

GH_VERSION="2.63.2"

install_gh() {
    info "Installing gh ${GH_VERSION}..."
    local tmp
    tmp=$(mktemp -d)
    curl --retry 3 --connect-timeout 10 -sSfL \
        "https://github.com/cli/cli/releases/download/v${GH_VERSION}/gh_${GH_VERSION}_linux_${ARCH_LABEL}.tar.gz" \
        -o "$tmp/gh.tar.gz"
    tar -xzf "$tmp/gh.tar.gz" -C "$tmp"
    cp "$tmp"/gh_*/bin/gh "$INSTALL_DIR/gh"
    chmod +x "$INSTALL_DIR/gh"
    rm -rf "$tmp"
    info "gh installed: $(gh --version | head -1)"
}

# ─── Install doppler ────────────────────────────────────────

install_doppler() {
    info "Installing doppler..."
    curl --retry 3 --connect-timeout 10 -sSfL https://cli.doppler.com/install.sh | sh
    info "doppler installed: $(doppler --version)"
}

# ─── Run installations in parallel ──────────────────────────

install_just &
install_gh &
install_doppler &
wait

# ─── Configure authentication ───────────────────────────────

# Doppler
if [[ -n "${DOPPLER_TOKEN:-}" ]]; then
    info "Configuring Doppler..."
    echo "$DOPPLER_TOKEN" | doppler configure set token --scope / 2>/dev/null || true
fi

# GitHub CLI
GH_TOKEN="${GH_TOKEN:-}"
if [[ -z "$GH_TOKEN" ]] && command -v doppler &>/dev/null; then
    GH_TOKEN=$(doppler secrets get GITHUB_TOKEN --plain 2>/dev/null || true)
fi

if [[ -n "$GH_TOKEN" ]]; then
    info "Configuring gh authentication..."
    echo "$GH_TOKEN" | gh auth login --with-token 2>/dev/null || true
fi

# Git remote configuration
if [[ -n "${GITHUB_REPOSITORY:-}" ]]; then
    gh repo set-default "$GITHUB_REPOSITORY" 2>/dev/null || true
fi

info "Cloud environment initialized."
