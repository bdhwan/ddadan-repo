import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

interface NgswConfig {
  dataGroups?: Array<{ urls?: string[] }>;
}

describe('coupon PWA cache policy', () => {
  it.each(['coupon-consumer-app', 'coupon-store-app'])(
    '%s caches only explicit public read paths and never QR or mutation responses',
    (app) => {
      const path = resolve(process.cwd(), `../../apps/${app}/ngsw-config.json`);
      const raw = readFileSync(path, 'utf8');
      const config = JSON.parse(raw) as NgswConfig;
      const dataUrls = config.dataGroups?.flatMap((group) => group.urls ?? []) ?? [];

      expect(dataUrls.length).toBeGreaterThan(0);
      expect(dataUrls.every((url) => url.startsWith('/api/coupon/v1/public/'))).toBe(true);
      expect(raw).not.toMatch(/qr-tokens|\/owner\/|\/me\/|\/campaigns\/.*claims/i);
    },
  );
});
