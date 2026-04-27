import { execSync } from 'child_process';
import { existsSync, readFileSync } from 'fs';
import { hostname } from 'os';

export function detectHardwareId(override: string | undefined): string {
  if (override && override.trim()) return override.trim();

  if (existsSync('/etc/machine-id')) {
    const id = readFileSync('/etc/machine-id', 'utf8').trim();
    if (id) return `mid-${id}`;
  }

  try {
    const mac = execSync('cat /sys/class/net/eth0/address', { encoding: 'utf8' }).trim();
    if (mac) return `mac-${mac.replace(/:/g, '')}`;
  } catch {
    // ignore
  }

  try {
    const mac = execSync('cat /sys/class/net/wlan0/address', { encoding: 'utf8' }).trim();
    if (mac) return `mac-${mac.replace(/:/g, '')}`;
  } catch {
    // ignore
  }

  return `host-${hostname()}`;
}
