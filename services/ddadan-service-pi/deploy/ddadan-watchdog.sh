#!/usr/bin/env bash
#
# ddadan-watchdog.sh — run every 5 minutes from the user's crontab.
#
# Guarantees the kiosk is up and on screen:
#   1. the systemd --user service must be active   → else restart it.
#   2. a chromium --kiosk process must exist        → else restart the service
#      (covers a frozen/disappeared browser = "전체화면 재실행").
#   3. re-assert outputs powered on / blanking off each pass.
#   4. kanshi must not pin an output mode the monitor cannot display → else fix.
#
# Runs as the display user (uid 1000) via crontab, so it shares the user's
# systemd manager and Wayland session.
#
# Maintenance mode: `touch ~/.ddadan-maintenance` to stop the watchdog from
# resurrecting an intentionally-stopped kiosk (debugging, demos). Remove the
# file to resume normal supervision. No crontab editing needed.

set -u

SERVICE="ddadan-pi.service"
LOGFILE="$HOME/.local/state/ddadan-watchdog.log"
MAXLOG=200000  # ~200KB then truncate
MAINT_FLAG="$HOME/.ddadan-maintenance"

KANSHI_CFG="${XDG_CONFIG_HOME:-$HOME/.config}/kanshi/config"
# Mode to fall back on when EDID is unreadable (see §4). 1920x1080 is the widest
# safe bet: it is in the no-EDID VESA fallback list AND is a mode essentially every
# HDMI panel will wake for. The refresh rate is deliberately NOT pinned — kanshi
# picks a matching one, so this survives panels that only offer 1080p50 or 1080p30.
SIGNAGE_MODE="${DDADAN_SIGNAGE_MODE:-1920x1080}"
DEFAULT_OUTPUT="${DDADAN_OUTPUT:-HDMI-A-1}"
KANSHI_GUARD="${DDADAN_KANSHI_GUARD:-1}"   # 0 disables §4 entirely (manual mode pinning)

UID_NUM="$(id -u)"
: "${XDG_RUNTIME_DIR:=/run/user/${UID_NUM}}"
export XDG_RUNTIME_DIR
: "${WAYLAND_DISPLAY:=wayland-0}"
export WAYLAND_DISPLAY
# cron has a minimal env; systemctl --user needs the user bus address.
: "${DBUS_SESSION_BUS_ADDRESS:=unix:path=${XDG_RUNTIME_DIR}/bus}"
export DBUS_SESSION_BUS_ADDRESS

mkdir -p "$(dirname "$LOGFILE")" 2>/dev/null || true

# Maintenance mode: leave everything alone (including a stopped service).
if [ -f "$MAINT_FLAG" ]; then
  echo "$(date '+%Y-%m-%d %H:%M:%S') maintenance flag present — skipping" >>"$LOGFILE" 2>/dev/null || true
  exit 0
fi

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
#    NOTE: we intentionally do NOT cycle the output off/on here even if wlopm
#    reports it stuck "off" — toggling wlr-randr --output X --off/--on tears
#    down and re-creates the wl_output global, and wayvnc treats that as the
#    monitor disappearing ("Selected output HDMI-A-1 went away" / "No fallback
#    outputs left. Exiting"), killing the RPi Connect remote view every time
#    this ran. A plain `wlopm --on` re-asserts DPMS power without doing that.
if command -v wlopm >/dev/null 2>&1; then
  wlopm --on '*' >/dev/null 2>&1 || true
fi
if [ -z "${WAYLAND_DISPLAY:-}" ] && command -v xset >/dev/null 2>&1; then
  xset s off 2>/dev/null; xset s noblank 2>/dev/null; xset -dpms 2>/dev/null
fi

# 4. kanshi mode guard — breaks the "monitor asleep at boot" deadlock.
#
#    force-hdmi.sh appends `video=HDMI-A-1:...D` to cmdline.txt, forcing the connector
#    on whether or not a monitor answers. That is what makes the following self-
#    sustaining and, without this guard, permanent:
#
#      monitor is in standby when the Pi boots
#        → it never answers on DDC, so EDID reads back empty
#        → the `D` flag marks the connector "connected" anyway (status is a lie here)
#        → with no EDID there is no preferred mode, so DRM offers only the generic
#          VESA fallback list: 1920x1080 / 1024x768 / 800x600 / 848x480 / 640x480
#        → the compositor settles on one of the junk ones, and if the Pi screen
#          configuration tool ever ran it is now frozen into ~/.config/kanshi/config
#        → 1024x768@60 and 800x600@56 are DMT modes, not CEA: the panel answers
#          "no signal" / "unsupported format" and STAYS ASLEEP
#        → so it still never answers on DDC. Round and round.
#
#    Chromium renders perfectly the whole time; only the wire is wrong. The way out
#    is to emit a mode the panel will accept, which wakes it, which makes DDC answer,
#    which lets the next boot read the real EDID and pick the real preferred mode.
#
#    The trigger is the LIVE MODE, not EDID. That distinction is load-bearing: these
#    panels answer DDC at boot and then stop — EDID reads back empty within the hour
#    while the picture stays up perfectly, because the compositor latched its mode at
#    session start and never needs EDID again. So "EDID is empty" does NOT mean the
#    screen is black; a guard keyed on it will happily downgrade a working 4K panel to
#    1080p. Only a junk mode on the wire means a black screen.
#
#      live mode is junk  → pin $SIGNAGE_MODE. This is the deadlock, and pinning a mode
#                           the panel accepts is the only way out of it.
#      EDID readable and
#      something is pinned → retire the pin and hand the mode back to the monitor, which
#                           knows better. Without this a 4K panel stays stranded at 1080p
#                           forever once its EDID returns.
#      anything else      → hands off. A healthy mode is healthy no matter what EDID says.
#
#    Note the session recreates an EMPTY ~/.config/kanshi/config on every boot (see
#    /etc/xdg/labwc/autostart). Empty pins nothing, so it is the healthy state and
#    must not be treated as a fault — else this would thrash on every pass.

# NB: sysfs binary attributes report st_size 0 even when populated, so the content
# has to actually be read. `stat -c%s` / `[ -s ]` on this path is a trap.
edid_present() {
  for _e in /sys/class/drm/card*-HDMI*/edid /sys/class/drm/card*-DP*/edid \
            /sys/class/drm/card*-eDP*/edid; do
    [ -f "$_e" ] || continue
    [ "$(cat "$_e" 2>/dev/null | wc -c)" -gt 0 ] && return 0
  done
  return 1
}

# The no-EDID VESA fallback list is 1920x1080 / 1024x768 / 800x600 / 848x480 / 640x480.
# Everything under 1280x720 in it is a DMT mode an HDMI panel will refuse. 1920x1080 is
# in the list too and is perfectly displayable, so landing there needs no repair.
mode_is_junk() {
  _w="${1%x*}"; _h="${1#*x}"
  [ -n "$_w" ] && [ -n "$_h" ] || return 1
  [ "$_w" -lt 1280 ] || [ "$_h" -lt 720 ]
}

restart_kanshi() {  # kanshi does the modeset itself. Unlike `wlr-randr --output --off/--on`
  pkill -x kanshi 2>/dev/null || true   # this never tears down the wl_output global, so
  sleep 1                               # wayvnc / RPi Connect survives it (see §3 above).
  setsid kanshi >/dev/null 2>&1 </dev/null &
}

if [ "$KANSHI_GUARD" = "1" ] && command -v kanshi >/dev/null 2>&1 && command -v wlr-randr >/dev/null 2>&1; then
  PINNED="$(grep -oE 'mode[[:space:]]+[0-9]+x[0-9]+' "$KANSHI_CFG" 2>/dev/null | awk '{print $2}' | head -1)"
  # Always take the output name from the compositor, never a hardcoded default: the
  # monitor may be on HDMI-A-2 (the Pi's second micro-HDMI port), and a pin written for
  # the wrong connector silently never applies.
  OUT="$(wlr-randr 2>/dev/null | grep -oE '^[A-Za-z0-9-]+' | head -1)"
  : "${OUT:=$DEFAULT_OUTPUT}"
  LIVE="$(wlr-randr 2>/dev/null | grep -i 'current' | grep -oE '[0-9]+x[0-9]+' | head -1)"

  if [ -n "$LIVE" ] && mode_is_junk "$LIVE"; then
    # Black screen: the panel is being fed a mode it will not sync to.
    if [ "$PINNED" != "$SIGNAGE_MODE" ]; then
      mkdir -p "$(dirname "$KANSHI_CFG")" 2>/dev/null || true
      [ ! -f "$KANSHI_CFG" ] || [ -f "$KANSHI_CFG.ddadan-bak" ] || cp "$KANSHI_CFG" "$KANSHI_CFG.ddadan-bak" 2>/dev/null || true
      printf 'profile {\n\toutput %s enable scale 1.000000 mode %s position 0,0 transform normal\n}\n' \
        "$OUT" "$SIGNAGE_MODE" >"$KANSHI_CFG"
      note "$OUT is on ${LIVE}, a mode the panel will not display (black screen) — pinned $SIGNAGE_MODE to wake it"
      restart_kanshi
    fi
  elif [ -n "$PINNED" ] && edid_present; then
    # The monitor is answering and the picture is fine, so the pin has outlived its job.
    [ -f "$KANSHI_CFG.ddadan-bak" ] || cp "$KANSHI_CFG" "$KANSHI_CFG.ddadan-bak" 2>/dev/null || true
    : >"$KANSHI_CFG"
    restart_kanshi
    # Clearing the pin only stops kanshi re-applying it; the output STAYS on whatever mode
    # was last set until something re-drives it (labwc reads the preferred mode only at
    # session start). So snap it back explicitly. `--preferred` is a plain modeset — it does
    # NOT destroy and re-create the wl_output the way `--off/--on` does, so wayvnc / RPi
    # Connect survives it (see §3).
    wlr-randr --output "$OUT" --preferred >/dev/null 2>&1 || true
    note "EDID is readable but kanshi still pinned ${PINNED} — cleared the pin and restored ${OUT} to its preferred mode (backup: $KANSHI_CFG.ddadan-bak)"
  fi
fi

if [ -n "$NEED" ]; then
  restart_service "$NEED"
else
  echo "$(date '+%Y-%m-%d %H:%M:%S') ok" >>"$LOGFILE" 2>/dev/null || true
fi
exit 0
