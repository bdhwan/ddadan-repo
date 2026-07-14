#!/usr/bin/env bash
#
# reboot-displays.sh — reboot the DDADAN Pi displays over SSH and wait for
# them to come back.
#
# Usage:
#   ./reboot-displays.sh                # reboot all three
#   ./reboot-displays.sh display-2      # reboot only display-2
#   ./reboot-displays.sh display-1 display-3
#
# Requires: ssh host aliases display-1/2/3 (see ~/.ssh/config) and
# passwordless sudo on each Pi.

set -u

HOSTS=("$@")
[ ${#HOSTS[@]} -gt 0 ] || HOSTS=(display-1 display-2 display-3)

echo "rebooting: ${HOSTS[*]}"

for h in "${HOSTS[@]}"; do
  if ssh -o ConnectTimeout=5 -o BatchMode=yes "$h" "sudo reboot" 2>/dev/null; then
    echo "  $h: reboot issued"
  else
    # sudo reboot drops the connection, which ssh reports as an error — only
    # treat it as a failure if the host wasn't reachable at all.
    if ssh -o ConnectTimeout=5 -o BatchMode=yes "$h" "true" 2>/dev/null; then
      echo "  $h: reboot command failed (sudo?)"
    else
      echo "  $h: reboot issued"
    fi
  fi
done

echo "waiting for hosts to come back..."
DEADLINE=$(( $(date +%s) + 180 ))

for h in "${HOSTS[@]}"; do
  # give the host a moment to actually go down before probing
  sleep 5
  while :; do
    if ssh -o ConnectTimeout=3 -o BatchMode=yes "$h" "uptime | grep -q 'up [0-9] min\|up 0'" 2>/dev/null; then
      echo "  $h: back online ($(ssh -o ConnectTimeout=3 "$h" "uptime | grep -oE 'up [^,]+'" 2>/dev/null))"
      break
    fi
    if [ "$(date +%s)" -ge "$DEADLINE" ]; then
      echo "  $h: TIMEOUT — not back within 180s"
      break
    fi
    sleep 3
  done
done

echo "done."
