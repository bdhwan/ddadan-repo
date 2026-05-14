import 'dotenv/config';

export interface PiConfig {
  apiBase: string;
  adminBase: string;
  playerBase: string;
  heartbeatIntervalSec: number;
  hardwareIdOverride: string | undefined;
  browserBin: string | undefined;
  launchKiosk: boolean;
  appVersion: string;
}

const intEnv = (v: string | undefined, fallback: number) => {
  const n = Number(v);
  return Number.isFinite(n) && v !== undefined && v !== '' ? n : fallback;
};

export function loadConfig(): PiConfig {
  return {
    apiBase: process.env.DDADAN_API_BASE ?? 'http://localhost:7800/api',
    adminBase: process.env.DDADAN_ADMIN_BASE ?? 'http://localhost:4200',
    playerBase: process.env.DDADAN_PLAYER_BASE ?? 'http://localhost:4300',
    heartbeatIntervalSec: intEnv(process.env.DDADAN_HEARTBEAT_INTERVAL_S, 20),
    hardwareIdOverride: process.env.DDADAN_HARDWARE_ID || undefined,
    browserBin: process.env.DDADAN_BROWSER_BIN || undefined,
    launchKiosk: process.env.DDADAN_LAUNCH_KIOSK === '1',
    appVersion: process.env.DDADAN_APP_VERSION ?? '0.1.0',
  };
}
