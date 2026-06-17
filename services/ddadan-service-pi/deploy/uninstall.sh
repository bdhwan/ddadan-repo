#!/usr/bin/env bash
#
# uninstall.sh — remove the DDADAN Pi kiosk user service + watchdog cron and
# restore the previous labwc autostart. Run as the desktop user.
#
set -euo pipefail

UNIT_DST="$HOME/.config/systemd/user/ddadan-pi.service"
AUTOSTART="$HOME/.config/labwc/autostart"

systemctl --user disable --now ddadan-pi.service 2>/dev/null || true
rm -f "$UNIT_DST"
systemctl --user daemon-reload 2>/dev/null || true

# Drop the watchdog cron line.
TMP="$(mktemp)"; crontab -l 2>/dev/null | grep -v 'ddadan-watchdog.sh' >"$TMP" || true
crontab "$TMP" 2>/dev/null || true; rm -f "$TMP"

# Restore the original labwc autostart if we backed it up.
if [ -f "$AUTOSTART.ddadan-bak" ]; then
  mv "$AUTOSTART.ddadan-bak" "$AUTOSTART"
  echo "[uninstall] restored $AUTOSTART from backup"
fi

echo "[uninstall] removed user service + watchdog cron. (.env and build left in place)"
echo "[uninstall] note: reboot or re-login to relaunch the restored autostart."
