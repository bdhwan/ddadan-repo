import { execSync } from 'child_process';

export interface MonitorReport {
  slot: number;
  resolutionW: number;
  resolutionH: number;
}

export function detectMonitors(): MonitorReport[] {
  const reports: MonitorReport[] = [];
  try {
    const out = execSync('xrandr --query', { encoding: 'utf8', timeout: 2000 });
    const lines = out.split('\n');
    let slot = 0;
    for (const line of lines) {
      const m = /\sconnected.*?(\d+)x(\d+)\+/.exec(line);
      if (m && m[1] && m[2]) {
        reports.push({
          slot,
          resolutionW: Number(m[1]),
          resolutionH: Number(m[2]),
        });
        slot += 1;
        if (slot >= 2) break;
      }
    }
  } catch {
    // xrandr unavailable (dev machine, headless); no-op.
  }

  if (reports.length === 0) {
    reports.push({ slot: 0, resolutionW: 1920, resolutionH: 1080 });
  }
  return reports;
}
