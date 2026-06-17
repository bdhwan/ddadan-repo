#!/usr/bin/env bash
#
# force-hdmi.sh — force the HDMI output on so a monitor connected at ANY time
# (even after a headless boot) is detected and displayed.
#
# Adds `video=<CONNECTOR>:<MODE>@<HZ>D` to /boot/firmware/cmdline.txt. The `D`
# flag forces the connector enabled even when nothing is detected at boot,
# which fixes "모니터를 연결해도 안 뜸" on full-KMS Raspberry Pi (Debian/labwc).
#
# Must run as root. Idempotent (won't add twice). Backs up cmdline.txt once.
#
# Usage:
#   sudo bash force-hdmi.sh                 # apply (no reboot)
#   sudo bash force-hdmi.sh reboot          # apply, then reboot
#   sudo CONNECTOR=HDMI-A-2 MODE=1920x1080 HZ=60 bash force-hdmi.sh
#
set -euo pipefail

CMDLINE="/boot/firmware/cmdline.txt"
[ -f "$CMDLINE" ] || CMDLINE="/boot/cmdline.txt"
[ -f "$CMDLINE" ] || { echo "ERROR: cmdline.txt not found"; exit 1; }
[ "$(id -u)" -eq 0 ] || { echo "ERROR: run with sudo"; exit 1; }

CONNECTOR="${CONNECTOR:-HDMI-A-1}"
MODE="${MODE:-1920x1080}"
HZ="${HZ:-60}"
PARAM="video=${CONNECTOR}:${MODE}@${HZ}D"

cp -n "$CMDLINE" "${CMDLINE}.bak"

if grep -q "video=${CONNECTOR}" "$CMDLINE"; then
  echo "already forced for ${CONNECTOR}; no change."
else
  # cmdline.txt is a single line — append the param at end of line.
  sed -i "s|\$| ${PARAM}|" "$CMDLINE"
  echo "added: ${PARAM}"
fi

echo "RESULT:"; cat "$CMDLINE"
echo "BACKUP: ${CMDLINE}.bak"

if [ "${1:-}" = "reboot" ]; then
  echo "rebooting in 3s..."; sleep 3; reboot
fi
