#!/usr/bin/env bash
#
# install.sh — install the DDADAN Pi kiosk as a supervised systemd *user*
# service + 5-minute watchdog cron, on a Raspberry Pi OS / Debian Wayland
# (labwc) session. Fully sudo-free: Node is installed into the user's home and
# the service runs in the autologin user session.
#
# Run as the DESKTOP user (NOT root):
#   cd services/ddadan-service-pi && ./deploy/install.sh
#
# Overridable env:
#   DDADAN_SERVER_IP   server (display-1) IP for API/player/admin (default 100.87.216.19)
#   DDADAN_NODE_MAJOR  node major to ensure (default 24)
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APPDIR="$(cd "$SCRIPT_DIR/.." && pwd)"
SERVER_IP="${DDADAN_SERVER_IP:-100.87.216.19}"
NODE_MAJOR="${DDADAN_NODE_MAJOR:-24}"
UNIT_DIR="$HOME/.config/systemd/user"
UNIT_DST="$UNIT_DIR/ddadan-pi.service"
NODE_HOME="$HOME/.local/lib/nodejs"

log()  { echo "[install] $*"; }
fail() { echo "[install] ERROR: $*" >&2; exit 1; }

[ "$(id -u)" -ne 0 ] || fail "run as the desktop user, NOT root"

# Make `systemctl --user` reachable even over a non-interactive SSH session.
UID_NUM="$(id -u)"
: "${XDG_RUNTIME_DIR:=/run/user/${UID_NUM}}"; export XDG_RUNTIME_DIR
: "${DBUS_SESSION_BUS_ADDRESS:=unix:path=${XDG_RUNTIME_DIR}/bus}"; export DBUS_SESSION_BUS_ADDRESS

# --- 1. ensure Node >= $NODE_MAJOR (user-local, no sudo) ----------------------
node_major() { "$1" -p 'process.versions.node.split(".")[0]' 2>/dev/null || echo 0; }
NODE_BIN=""
if command -v node >/dev/null 2>&1 && [ "$(node_major node)" -ge "$NODE_MAJOR" ]; then
  NODE_BIN="$(command -v node)"
elif [ -x "$NODE_HOME/bin/node" ] && [ "$(node_major "$NODE_HOME/bin/node")" -ge "$NODE_MAJOR" ]; then
  NODE_BIN="$NODE_HOME/bin/node"
else
  log "installing Node ${NODE_MAJOR}.x into $NODE_HOME (no sudo) ..."
  case "$(uname -m)" in
    aarch64|arm64) NARCH="arm64" ;;
    armv7l|armv6l) NARCH="armv7l" ;;
    x86_64)        NARCH="x64" ;;
    *) fail "unsupported arch $(uname -m)" ;;
  esac
  VER="$(curl -fsSL https://nodejs.org/dist/index.json | grep -o "\"version\":\"v${NODE_MAJOR}[^\"]*\"" | head -1 | cut -d'"' -f4)"
  [ -n "$VER" ] || fail "could not resolve latest Node ${NODE_MAJOR} version"
  TARBALL="node-${VER}-linux-${NARCH}.tar.xz"
  log "downloading $TARBALL ..."
  mkdir -p "$NODE_HOME"
  curl -fsSL "https://nodejs.org/dist/${VER}/${TARBALL}" -o "/tmp/${TARBALL}"
  tar -xJf "/tmp/${TARBALL}" -C "$NODE_HOME" --strip-components=1
  rm -f "/tmp/${TARBALL}"
  NODE_BIN="$NODE_HOME/bin/node"
  # Make node/npm available in interactive shells too.
  if ! grep -q 'ddadan node path' "$HOME/.profile" 2>/dev/null; then
    printf '\n# ddadan node path\nexport PATH="%s/bin:$PATH"\n' "$NODE_HOME" >>"$HOME/.profile"
  fi
fi
NPM_BIN="$(dirname "$NODE_BIN")/npm"
log "node=$NODE_BIN ($("$NODE_BIN" --version))"

# --- 2. build ----------------------------------------------------------------
export PATH="$(dirname "$NODE_BIN"):$PATH"
log "building in $APPDIR ..."
( cd "$APPDIR" && { [ -f package-lock.json ] && "$NPM_BIN" ci || "$NPM_BIN" install; } )
( cd "$APPDIR" && "$NPM_BIN" run build )
[ -f "$APPDIR/dist/index.js" ] || fail "build did not produce dist/index.js"

# --- 3. .env -----------------------------------------------------------------
ENV_FILE="$APPDIR/.env"
[ -f "$ENV_FILE" ] || cp "$APPDIR/.env.example" "$ENV_FILE"
set_env() {
  if grep -q "^$1=" "$ENV_FILE"; then sed -i "s|^$1=.*|$1=$2|" "$ENV_FILE"; else echo "$1=$2" >>"$ENV_FILE"; fi
}
set_env DDADAN_API_BASE    "http://${SERVER_IP}:7800/api"
set_env DDADAN_PLAYER_BASE "http://${SERVER_IP}:7300"
set_env DDADAN_ADMIN_BASE  "http://${SERVER_IP}:4200"
set_env DDADAN_LAUNCH_KIOSK 1
set_env DDADAN_KIOSK_LAUNCHER "$SCRIPT_DIR/kiosk-launch.sh"
log ".env configured for server ${SERVER_IP}"

# --- 4. executables ----------------------------------------------------------
chmod +x "$SCRIPT_DIR/kiosk-launch.sh" "$SCRIPT_DIR/ddadan-watchdog.sh"

# --- 5. systemd user service -------------------------------------------------
mkdir -p "$UNIT_DIR"
sed -e "s|__APPDIR__|$APPDIR|g" -e "s|__NODE__|$NODE_BIN|g" \
  "$SCRIPT_DIR/ddadan-pi.service" >"$UNIT_DST"
systemctl --user daemon-reload
systemctl --user enable ddadan-pi.service
# Best-effort: keep user manager alive without an active login (needs sudo).
sudo -n loginctl enable-linger "$(id -un)" 2>/dev/null && log "enabled linger" \
  || log "note: linger not enabled (autologin keeps the session active anyway)"
log "user service installed + enabled"

# --- 6. neutralize the old inline labwc kiosk loop ---------------------------
AUTOSTART="$HOME/.config/labwc/autostart"
if [ -f "$AUTOSTART" ] && grep -q "chromium" "$AUTOSTART" 2>/dev/null; then
  [ -f "$AUTOSTART.ddadan-bak" ] || cp "$AUTOSTART" "$AUTOSTART.ddadan-bak"
  cat >"$AUTOSTART" <<EOF
#!/bin/bash
# DDADAN kiosk is now managed by the systemd user service (ddadan-pi.service).
# Old inline launcher backed up at: $AUTOSTART.ddadan-bak
command -v wlopm >/dev/null 2>&1 && wlopm --on '*' >/dev/null 2>&1 || true
systemctl --user import-environment WAYLAND_DISPLAY XDG_RUNTIME_DIR XDG_SESSION_TYPE 2>/dev/null || true
systemctl --user restart ddadan-pi.service 2>/dev/null || true
EOF
  log "replaced inline labwc kiosk with service starter (backup kept)"
fi

# --- 7. 5-minute watchdog cron (user crontab, idempotent) --------------------
CRON_LINE="*/5 * * * * $SCRIPT_DIR/ddadan-watchdog.sh"
TMPCRON="$(mktemp)"
crontab -l 2>/dev/null | grep -v 'ddadan-watchdog.sh' >"$TMPCRON" || true
echo "$CRON_LINE" >>"$TMPCRON"
crontab "$TMPCRON"; rm -f "$TMPCRON"
log "watchdog cron installed: $CRON_LINE"

# --- 8. start now ------------------------------------------------------------
systemctl --user restart ddadan-pi.service || true

echo
log "DONE."
systemctl --user --no-pager status ddadan-pi.service 2>/dev/null | head -12 || true
echo
log "Logs:         journalctl --user -u ddadan-pi -f"
log "Watchdog log: tail -f ~/.local/state/ddadan-watchdog.log"
log "Edit config:  $ENV_FILE  (then: systemctl --user restart ddadan-pi)"
