import { hostname } from 'os';

export function detectHardwareId(override: string | undefined): string {
  if (override && override.trim()) return override.trim();
  return hostname();
}
