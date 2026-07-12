import { execSync } from 'child_process';

export interface MonitorReport {
  slot: number;
  resolutionW: number;
  resolutionH: number;
}

// Parse `wlr-randr` output (Wayland/labwc — the normal case on the Pi).
// Output names start at column 0; the active mode line is indented and
// marked "(current)":
//   HDMI-A-1 "..."
//     ...
//       1920x1080 px, 59.993999 Hz (current)
function parseWlrRandr(out: string): MonitorReport[] {
  const reports: MonitorReport[] = [];
  let slot = 0;
  let inOutput = false;
  for (const line of out.split('\n')) {
    if (/^\S/.test(line)) {
      inOutput = true;
      continue;
    }
    if (!inOutput) continue;
    const m = /(\d+)x(\d+)\s*px.*\(current\)/.exec(line);
    if (m && m[1] && m[2]) {
      reports.push({ slot, resolutionW: Number(m[1]), resolutionH: Number(m[2]) });
      slot += 1;
      inOutput = false; // one current mode per output
      if (slot >= 2) break;
    }
  }
  return reports;
}

// Parse `xrandr --query` output (X11 fallback).
function parseXrandr(out: string): MonitorReport[] {
  const reports: MonitorReport[] = [];
  let slot = 0;
  for (const line of out.split('\n')) {
    const m = /\sconnected.*?(\d+)x(\d+)\+/.exec(line);
    if (m && m[1] && m[2]) {
      reports.push({ slot, resolutionW: Number(m[1]), resolutionH: Number(m[2]) });
      slot += 1;
      if (slot >= 2) break;
    }
  }
  return reports;
}

export function detectMonitors(): MonitorReport[] {
  // Wayland first: xrandr silently reports nothing (or errors) under labwc,
  // which used to make every device report the 1920x1080 fallback regardless
  // of the real mode.
  try {
    const out = execSync('wlr-randr', { encoding: 'utf8', timeout: 2000 });
    const reports = parseWlrRandr(out);
    if (reports.length > 0) return reports;
  } catch {
    // wlr-randr unavailable (X11 session, dev machine); try xrandr.
  }

  try {
    const out = execSync('xrandr --query', { encoding: 'utf8', timeout: 2000 });
    const reports = parseXrandr(out);
    if (reports.length > 0) return reports;
  } catch {
    // xrandr unavailable too (headless); fall through.
  }

  return [{ slot: 0, resolutionW: 1920, resolutionH: 1080 }];
}
