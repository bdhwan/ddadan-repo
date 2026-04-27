import { resolve } from 'path';

export interface AppConfig {
  port: number;
  env: string;
  corsOrigins: string[];
  db: {
    host: string;
    port: number;
    username: string;
    password: string;
    database: string;
    synchronize: boolean;
    logging: boolean;
  };
  redis: {
    host: string;
    port: number;
    password?: string;
  };
  firebase: {
    serviceAccountPath: string;
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

export const loadConfig = (): AppConfig => ({
  port: toInt(process.env.PORT, 3000),
  env: process.env.NODE_ENV ?? 'development',
  corsOrigins: (process.env.CORS_ORIGINS ?? 'http://localhost:4200')
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean),
  db: {
    host: process.env.DB_HOST ?? '127.0.0.1',
    port: toInt(process.env.DB_PORT, 3306),
    username: process.env.DB_USERNAME ?? 'ddadan',
    password: process.env.DB_PASSWORD ?? 'ddadan',
    database: process.env.DB_DATABASE ?? 'ddadan',
    synchronize: toBool(process.env.DB_SYNCHRONIZE, true),
    logging: toBool(process.env.DB_LOGGING, false),
  },
  redis: {
    host: process.env.REDIS_HOST ?? '127.0.0.1',
    port: toInt(process.env.REDIS_PORT, 6379),
    password: process.env.REDIS_PASSWORD || undefined,
  },
  firebase: {
    serviceAccountPath: resolve(
      process.cwd(),
      process.env.FIREBASE_SERVICE_ACCOUNT_PATH ??
        './firebase/service-account.json',
    ),
  },
  assets: {
    dir: resolve(process.cwd(), process.env.ASSETS_DIR ?? './storage/assets'),
    publicPath: process.env.ASSETS_PUBLIC_PATH ?? '/static/assets',
  },
  heartbeat: {
    offlineAfterSeconds: toInt(process.env.HEARTBEAT_OFFLINE_AFTER_SECONDS, 60),
  },
});

export type ConfigKey = keyof AppConfig;
