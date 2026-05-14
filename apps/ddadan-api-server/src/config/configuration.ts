import { homedir } from 'os';
import { join, resolve } from 'path';

export interface AppConfig {
  host: string;
  port: number;
  env: string;
  db: {
    sqlitePath: string;
    synchronize: boolean;
    logging: boolean;
  };
  assets: {
    dir: string;
    publicPath: string;
  };
  heartbeat: {
    offlineAfterSeconds: number;
  };
}

const toBool = (v: string | undefined, fallback: boolean) =>
  v === undefined ? fallback : v === 'true' || v === '1';

const toInt = (v: string | undefined, fallback: number) => {
  const n = Number(v);
  return Number.isFinite(n) && v !== undefined && v !== '' ? n : fallback;
};

const defaultAssetsDir = () => join(homedir(), 'Desktop', 'resource');

export const loadConfig = (): AppConfig => {
  const assetsDir = process.env.ASSETS_DIR
    ? resolve(process.cwd(), process.env.ASSETS_DIR)
    : defaultAssetsDir();

  const sqlitePath = process.env.DB_PATH
    ? resolve(process.cwd(), process.env.DB_PATH)
    : join(assetsDir, 'ddadan.sqlite');

  return {
    host: process.env.HOST ?? '0.0.0.0',
    port: toInt(process.env.PORT, 7800),
    env: process.env.NODE_ENV ?? 'development',
    db: {
      sqlitePath,
      synchronize: toBool(process.env.DB_SYNCHRONIZE, true),
      logging: toBool(process.env.DB_LOGGING, false),
    },
    assets: {
      dir: assetsDir,
      publicPath: process.env.ASSETS_PUBLIC_PATH ?? '/static/assets',
    },
    heartbeat: {
      offlineAfterSeconds: toInt(process.env.HEARTBEAT_OFFLINE_AFTER_SECONDS, 60),
    },
  };
};

export type ConfigKey = keyof AppConfig;
