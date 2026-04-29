#!/usr/bin/env bash
set -euo pipefail

REPO="kristijanribaric/wl-keypeek"
BINARY_NAME="keypeek-wayland"
INSTALL_DIR="$HOME/.local/bin"
SERVICE_NAME="keypeek-wayland.service"
SERVICE_DIR="$HOME/.config/systemd/user"
UDEV_RULE_DST="/etc/udev/rules.d/99-keypeek.rules"

# ── colours ──────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
BOLD='\033[1m'; RESET='\033[0m'

info()    { echo -e "${BOLD}  →  $*${RESET}"; }
success() { echo -e "${GREEN}  ✓  $*${RESET}"; }
warn()    { echo -e "${YELLOW}  ⚠  $*${RESET}"; }
error()   { echo -e "${RED}  ✗  $*${RESET}" >&2; exit 1; }

# ── helpers ───────────────────────────────────────────────────────────────────
require() {
  command -v "$1" &>/dev/null || error "Required tool '$1' not found. Please install it and re-run."
}

# ── banner ────────────────────────────────────────────────────────────────────
echo
echo -e "${BOLD}wl-KeyPeek installer${RESET}"
echo "────────────────────────────────────────"
echo

require curl
require jq

# ── fetch latest release ──────────────────────────────────────────────────────
info "Fetching latest release from GitHub..."

RELEASE_JSON=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest")
VERSION=$(echo "$RELEASE_JSON" | jq -r '.tag_name')
APPIMAGE_URL=$(echo "$RELEASE_JSON" | jq -r '.assets[] | select(.name | test("linux-x86_64\\.AppImage$")) | .browser_download_url')

[[ -z "$APPIMAGE_URL" ]] && error "Could not find a Linux AppImage in the latest release (${VERSION}). Check https://github.com/${REPO}/releases"

success "Found release ${VERSION}"

# ── download AppImage ─────────────────────────────────────────────────────────
TMPDIR_INST=$(mktemp -d)
trap 'rm -rf "$TMPDIR_INST"' EXIT

APPIMAGE_FILE="$TMPDIR_INST/wl-keypeek.AppImage"

info "Downloading AppImage..."
curl -fL --progress-bar "$APPIMAGE_URL" -o "$APPIMAGE_FILE"
chmod +x "$APPIMAGE_FILE"
success "Downloaded $(basename "$APPIMAGE_URL")"

# ── stop existing service if running ─────────────────────────────────────────
if systemctl --user is-active --quiet "$SERVICE_NAME" 2>/dev/null; then
  info "Stopping existing service..."
  systemctl --user stop "$SERVICE_NAME"
fi

# ── install binary ────────────────────────────────────────────────────────────
mkdir -p "$INSTALL_DIR"
info "Installing binary to ${INSTALL_DIR}/${BINARY_NAME}..."
cp "$APPIMAGE_FILE" "${INSTALL_DIR}/${BINARY_NAME}"
success "Binary installed"

# ── install systemd service ───────────────────────────────────────────────────
info "Installing systemd user service..."
mkdir -p "$SERVICE_DIR"
cat > "${SERVICE_DIR}/${SERVICE_NAME}" <<EOF
[Unit]
Description=KeyPeek Wayland Overlay
After=graphical-session.target
PartOf=graphical-session.target

[Service]
ExecStartPre=/bin/sleep 5
ExecStart=%h/.local/bin/${BINARY_NAME}
Restart=on-failure
RestartSec=3

[Install]
WantedBy=graphical-session.target
EOF
systemctl --user daemon-reload
success "Systemd service installed"

# ── udev rule (optional, for ZMK / hidraw access) ────────────────────────────
echo
echo -e "${BOLD}Udev rule (ZMK / hidraw access)${RESET}"
echo "  Required if you use a ZMK keyboard and the app can't see it."
read -rp "  Install udev rule? Requires sudo. [y/N] " INSTALL_UDEV </dev/tty
if [[ "${INSTALL_UDEV,,}" == "y" ]]; then
  info "Installing udev rule to ${UDEV_RULE_DST}..."
  echo 'SUBSYSTEM=="hidraw", ATTRS{idVendor}=="1d50", MODE="0660", GROUP="input"' \
    | sudo tee "$UDEV_RULE_DST" > /dev/null
  sudo udevadm control --reload-rules
  sudo udevadm trigger
  success "Udev rule installed — reconnect your keyboard if it was already plugged in"
else
  warn "Skipped. You can install it later with:"
  echo '    echo '"'"'SUBSYSTEM=="hidraw", ATTRS{idVendor}=="1d50", MODE="0660", GROUP="input"'"'"' | sudo tee /etc/udev/rules.d/99-keypeek.rules && sudo udevadm control --reload-rules && sudo udevadm trigger'
fi

# ── enable & start service ────────────────────────────────────────────────────
echo
info "Enabling and starting wl-KeyPeek service..."
systemctl --user enable --now "$SERVICE_NAME"
success "Service running"

# ── done ──────────────────────────────────────────────────────────────────────
echo
echo -e "${GREEN}${BOLD}  wl-KeyPeek ${VERSION} installed successfully!${RESET}"
echo
echo "  Manage the service:"
echo "    systemctl --user status  ${SERVICE_NAME}"
echo "    systemctl --user stop    ${SERVICE_NAME}"
echo "    systemctl --user restart ${SERVICE_NAME}"
echo "    journalctl --user -u     ${SERVICE_NAME} -f"
echo
echo "  To uninstall:"
echo "    systemctl --user disable --now ${SERVICE_NAME}"
echo "    rm ${INSTALL_DIR}/${BINARY_NAME}"
echo "    rm ${SERVICE_DIR}/${SERVICE_NAME}"
echo
