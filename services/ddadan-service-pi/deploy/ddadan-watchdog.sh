#!/usr/bin/env bash
#
# ddadan-watchdog.sh — run every 5 minutes from the user's crontab.
#
# Guarantees the kiosk is up and on screen:
#   1. the systemd --user service must be active   → else restart it.
#   2. a chromium --kiosk process must exist        → else restart the service
#      (covers a frozen/disappeared browser = "전체화면 재실행").
#   3. re-assert outputs powered on / blanking off each pass.
#
# Runs as the display user (uid 1000) via crontab, so it shares the user's
# systemd manager and Wayland session.

set -u

SERVICE="ddadan-pi.service"
LOGFILE="$HOME/.local/state/ddadan-watchdog.log"
MAXLOG=200000  # ~200KB then truncate

UID_NUM="$(id -u)"
: "${XDG_RUNTIME_DIR:=/run/user/${UID_NUM}}"
export XDG_RUNTIME_DIR
: "${WAYLAND_DISPLAY:=wayland-0}"
export WAYLAND_DISPLAY
# cron has a minimal env; systemctl --user needs the user bus address.
: "${DBUS_SESSION_BUS_ADDRESS:=unix:path=${XDG_RUNTIME_DIR}/bus}"
export DBUS_SESSION_BUS_ADDRESS

mkdir -p "$(dirname "$LOGFILE")" 2>/dev/null || true

note() {  # journal + capped logfile
  command -v logger >/dev/null 2>&1 && logger -t ddadan-watchdog -- "$*" || true
  if [ -f "$LOGFILE" ] && [ "$(wc -c <"$LOGFILE" 2>/dev/null || echo 0)" -gt "$MAXLOG" ]; then : >"$LOGFILE"; fi
  echo "$(date '+%Y-%m-%d %H:%M:%S') $*" >>"$LOGFILE" 2>/dev/null || true
}

restart_service() {
  note "restarting $SERVICE: $1"
  systemctl --user restart "$SERVICE" 2>/dev/null || note "systemctl --user restart failed"
}

# A real monitor is connected only if some DRM output reports "connected".
monitor_connected() {
  for s in /sys/class/drm/card*-HDMI*/status /sys/class/drm/card*-DP*/status \
           /sys/class/drm/card*-eDP*/status /sys/class/drm/card*-DVI*/status; do
    [ -f "$s" ] && [ "$(cat "$s" 2>/dev/null)" = "connected" ] && return 0
  done
  return 1
}

# 1. service active?
NEED=""
if ! systemctl --user is-active --quiet "$SERVICE"; then
  NEED="service not active"
fi

# 2. chromium kiosk present — ONLY required when a monitor is actually connected.
#    With no monitor the launcher is correctly idle-waiting, so missing chromium
#    is expected and must NOT trigger a restart.
if [ -z "$NEED" ] && monitor_connected && ! pgrep -f -- '--kiosk' >/dev/null 2>&1; then
  NEED="monitor connected but no chromium --kiosk process"
fi

# 3. keep outputs on / blanking off (cheap, idempotent, best-effort).
command -v wlopm >/dev/null 2>&1 && wlopm --on '*' >/dev/null 2>&1 || true
if [ -z "${WAYLAND_DISPLAY:-}" ] && command -v xset >/dev/null 2>&1; then
  xset s off 2>/dev/null; xset s noblank 2>/dev/null; xset -dpms 2>/dev/null
fi

if [ -n "$NEED" ]; then
  restart_service "$NEED"
else
  echo "$(date '+%Y-%m-%d %H:%M:%S') ok" >>"$LOGFILE" 2>/dev/null || true
fi
exit 0
