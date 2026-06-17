import { DdadanApi } from './api';
import { loadConfig, type PiConfig } from './config';
import { detectHardwareId } from './hardware-id';
import { KioskSupervisor } from './kiosk';
import { detectMonitors, type MonitorReport } from './monitors';

async function bootstrap() {
  const config: PiConfig = loadConfig();
  const hardwareId = detectHardwareId(config.hardwareIdOverride);
  const monitors: MonitorReport[] = detectMonitors();

  console.log(`[ddadan-pi] starting hardwareId=${hardwareId} monitors=${monitors.length}`);

  const api = new DdadanApi(config);
  const kiosk = new KioskSupervisor({
    browserBin: config.browserBin,
    launcher: config.kioskLauncher,
    enabled: config.launchKiosk,
  });

  await refreshTargetUrl({ api, config, hardwareId, monitors, kiosk });

  setInterval(async () => {
    try {
      await api.heartbeat(hardwareId, config.appVersion, monitors);
    } catch (err) {
      console.warn(`[ddadan-pi] heartbeat failed: ${(err as Error).message}`);
    }
    await refreshTargetUrl({ api, config, hardwareId, monitors, kiosk });
  }, config.heartbeatIntervalSec * 1000);

  process.on('SIGINT', () => {
    console.log('[ddadan-pi] SIGINT, shutting down');
    kiosk.shutdown();
    process.exit(0);
  });
  process.on('SIGTERM', () => {
    console.log('[ddadan-pi] SIGTERM, shutting down');
    kiosk.shutdown();
    process.exit(0);
  });
}

async function refreshTargetUrl(args: {
  api: DdadanApi;
  config: PiConfig;
  hardwareId: string;
  monitors: MonitorReport[];
  kiosk: KioskSupervisor;
}) {
  const { api, config, hardwareId, monitors, kiosk } = args;
  const playerUrl = `${config.playerBase}/?deviceId=${encodeURIComponent(hardwareId)}`;
  try {
    const result = await api.check(hardwareId, monitors);
    if (!result.registered) {
      const url = `${config.adminBase}/register?deviceId=${encodeURIComponent(hardwareId)}`;
      kiosk.setUrl(url);
    } else {
      kiosk.setUrl(playerUrl);
    }
  } catch (err) {
    console.warn(`[ddadan-pi] check failed: ${(err as Error).message}`);
    // Never leave the screen blank on a transient API error: if nothing is
    // showing yet, fall back to the player URL (the page itself tolerates an
    // offline API). An already-running kiosk keeps its current URL.
    if (!kiosk.hasUrl()) {
      console.warn('[ddadan-pi] no URL yet — falling back to player URL');
      kiosk.setUrl(playerUrl);
    }
  }
}

bootstrap().catch((err) => {
  console.error('[ddadan-pi] fatal:', err);
  process.exit(1);
});
