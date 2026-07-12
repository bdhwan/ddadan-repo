#!/usr/bin/env bash
#
# kiosk-launch.sh <url>
#
# Robust fullscreen Chromium launcher for the DDADAN Pi display.
# The Node service (kiosk.ts) execs this with the target URL as $1 and
# respawns it whenever it exits or the target URL changes.
#
# Primary target is Wayland/labwc (Raspberry Pi OS / Debian 13), with an X11
# fallback. Behaviour mirrors the proven labwc autostart launcher:
#   - Wait for a *real* monitor (HDMI/DP/eDP/VGA/DVI) to be connected.
#   - Disable screen blanking / keep outputs powered on.
#   - Launch chromium in kiosk mode at the detected resolution.
#   - Supervise it: if chromium dies OR the monitor is unplugged, clean up and
#     exit so the Node supervisor respawns us (→ back to waiting for a monitor).
#
# All optional tools are guarded by `command -v`, so a missing helper never
# aborts the kiosk.

set -u

URL="${1:-}"
if [ -z "$URL" ]; then
  echo "[kiosk-launch] no URL given" >&2
  exit 2
fi

POLL_SEC="${DDADAN_MONITOR_POLL_SEC:-3}"
REAL_OUTPUT_RE='^(HDMI|DP|eDP|VGA|DVI)'
CHILD_PID=""

log() { echo "[kiosk-launch] $*"; }
has() { command -v "$1" >/dev/null 2>&1; }

# ---------------------------------------------------------------------------
# Environment: make sure the Wayland/X11 session coordinates are present even
# when started by systemd (which has a minimal environment).
# ---------------------------------------------------------------------------
UID_NUM="$(id -u)"
: "${XDG_RUNTIME_DIR:=/run/user/${UID_NUM}}"
export XDG_RUNTIME_DIR

if [ -z "${WAYLAND_DISPLAY:-}" ] && [ -d "$XDG_RUNTIME_DIR" ]; then
  for sock in "$XDG_RUNTIME_DIR"/wayland-*; do
    case "$sock" in *.lock) continue ;; esac
    [ -S "$sock" ] && { WAYLAND_DISPLAY="$(basename "$sock")"; break; }
  done
fi

SERVER="x11"
if [ -n "${WAYLAND_DISPLAY:-}" ]; then
  SERVER="wayland"
  export WAYLAND_DISPLAY
else
  : "${DISPLAY:=:0}"
  export DISPLAY
  if [ -z "${XAUTHORITY:-}" ]; then
    for xauth in "$HOME/.Xauthority" "/home/$(id -un)/.Xauthority"; do
      [ -f "$xauth" ] && { XAUTHORITY="$xauth"; break; }
    done
  fi
  [ -n "${XAUTHORITY:-}" ] && export XAUTHORITY
fi
log "server=${SERVER} WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-} DISPLAY=${DISPLAY:-} XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR} url=${URL}"

# ---------------------------------------------------------------------------
# Clean shutdown: kill chromium when we are told to stop (URL change / service
# restart) so we never leave an orphaned browser behind.
# ---------------------------------------------------------------------------
cleanup() {
  if [ -n "$CHILD_PID" ] && kill -0 "$CHILD_PID" 2>/dev/null; then
    kill "$CHILD_PID" 2>/dev/null
    for _ in 1 2 3 4 5; do kill -0 "$CHILD_PID" 2>/dev/null || break; sleep 1; done
    kill -9 "$CHILD_PID" 2>/dev/null
  fi
  pkill -9 -f '/usr/lib/chromium' 2>/dev/null || true
}
trap 'cleanup; exit 0' TERM INT

# ---------------------------------------------------------------------------
# Disable screen blanking / keep all outputs powered on.
# ---------------------------------------------------------------------------
# NOTE: we used to cycle the connector (wlr-randr --output X --off then --on)
# when wlopm reported it stuck "off", to wake the physical HDMI signal. That
# tears down and re-creates the wl_output global, which wayvnc (RPi Connect's
# screen capture backend) treats as the monitor disappearing for good
# ("Selected output HDMI-A-1 went away" / "No fallback outputs left.
# Exiting"), killing the remote view. Just re-assert DPMS power instead.
disable_blanking() {
  if [ "$SERVER" = "wayland" ]; then
    has wlopm && wlopm --on '*' >/dev/null 2>&1 || true
  else
    if has xset; then xset s off 2>/dev/null; xset s noblank 2>/dev/null; xset -dpms 2>/dev/null; fi
  fi
}

# "<name>\t<WxH>" of the first connected real output, or empty.
real_output_info() {
  if [ "$SERVER" = "wayland" ]; then
    has wlr-randr || { echo ""; return; }
    wlr-randr 2>/dev/null | awk -v re="$REAL_OUTPUT_RE" '
      /^[^[:space:]]/ { cur=$1; matched=(cur ~ re)?1:0 }
      matched && /current/ { print cur "\t" $1; exit }'
  else
    has xrandr || { echo "auto\t1920x1080"; return; }
    xrandr --query 2>/dev/null | awk -v re="$REAL_OUTPUT_RE" '
      / connected/ && $1 ~ re { name=$1 }
      name && /\*/ { for(i=1;i<=NF;i++) if($i ~ /[0-9]+x[0-9]+/){print name "\t" $i; exit} }'
  fi
}

launch_chromium() {
  local size="$1" w h
  w="${size%x*}"; h="${size#*x}"
  local browser="${DDADAN_BROWSER_BIN:-}"
  if [ -z "$browser" ]; then
    for b in chromium chromium-browser google-chrome; do has "$b" && { browser="$b"; break; }; done
  fi
  [ -n "$browser" ] || { log "no chromium binary found"; return 1; }

  # Dedicated profile so we can scrub the crash flag without touching anything else.
  local prof="${DDADAN_KIOSK_PROFILE:-$HOME/.config/ddadan-kiosk}"
  mkdir -p "$prof/Default" 2>/dev/null || true
  local prefs="$prof/Default/Preferences"
  if [ -f "$prefs" ] && has sed; then
    sed -i 's/"exit_type":"[^"]*"/"exit_type":"Normal"/; s/"exited_cleanly":false/"exited_cleanly":true/' "$prefs" 2>/dev/null || true
  fi

  local args=(
    --kiosk --start-fullscreen --noerrdialogs --no-first-run
    --disable-translate --disable-infobars
    --disable-session-crashed-bubble --disable-features=Translate
    --disable-background-networking --check-for-update-interval=31536000
    --overscroll-history-navigation=0 --disable-pinch
    --password-store=basic
    --window-size="${w},${h}" --window-position=0,0
    --user-data-dir="$prof"
  )
  [ "$SERVER" = "wayland" ] && args+=(--ozone-platform=wayland --enable-features=UseOzonePlatform)

  log "launching: $browser (${w}x${h}) $URL"
  "$browser" "${args[@]}" "$URL" >/dev/null 2>&1 &
  CHILD_PID=$!
}

# ---------------------------------------------------------------------------
# Main: wait for monitor → launch → supervise → exit on death/disconnect.
# ---------------------------------------------------------------------------
disable_blanking

# 1) Wait until a real monitor is connected.
INFO=""
while :; do
  INFO="$(real_output_info)"
  [ -n "$INFO" ] && break
  log "no monitor connected — waiting..."
  sleep "$POLL_SEC"
done
NAME="${INFO%%	*}"; SIZE="${INFO##*	}"
[ -n "$SIZE" ] || SIZE="1920x1080"
log "monitor connected: $NAME $SIZE"

# 2) Launch chromium.
disable_blanking
launch_chromium "$SIZE" || exit 1

# 3) Supervise: exit (→ respawn) if chromium dies or the monitor disappears.
while kill -0 "$CHILD_PID" 2>/dev/null; do
  sleep "$POLL_SEC"
  if [ -z "$(real_output_info)" ]; then
    log "monitor disconnected — stopping chromium"
    cleanup
    exit 0
  fi
done
log "chromium exited — handing back to supervisor"
exit 0
