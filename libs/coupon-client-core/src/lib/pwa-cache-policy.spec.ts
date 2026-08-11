import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

interface NgswConfig {
  assetGroups?: Array<{ resources?: { files?: string[]; urls?: string[] } }>;
  dataGroups?: Array<{
    urls?: string[];
    cacheConfig?: { strategy?: string; maxAge?: string };
  }>;
}

describe("coupon PWA cache policy", () => {
  it.each(["coupon-consumer-app", "coupon-store-app"])(
    "%s caches only explicit public read paths and never QR or mutation responses",
    (app) => {
      const path = resolve(process.cwd(), `../../apps/${app}/ngsw-config.json`);
      const raw = readFileSync(path, "utf8");
      const config = JSON.parse(raw) as NgswConfig;
      const dataUrls =
        config.dataGroups?.flatMap((group) => group.urls ?? []) ?? [];

      expect(dataUrls.length).toBeGreaterThan(0);
      expect(
        dataUrls.every((url) => url.startsWith("/api/coupon/v1/public/")),
      ).toBe(true);
      expect(raw).not.toMatch(
        /qr-tokens|\/owner\/|\/me\/|\/admin\/|\/campaigns\/.*claims|redemptions|stamp-transactions/i,
      );
      expect(
        config.dataGroups?.every(
          (group) =>
            group.cacheConfig?.strategy === "freshness" &&
            Boolean(group.cacheConfig.maxAge),
        ),
      ).toBe(true);
      const assetUrls =
        config.assetGroups?.flatMap((group) => [
          ...(group.resources?.files ?? []),
          ...(group.resources?.urls ?? []),
        ]) ?? [];
      expect(assetUrls.some((url) => url.includes("/api/"))).toBe(false);
    },
  );

  it.each(["coupon-consumer-app", "coupon-store-app"])(
    "%s manifest is installable and localized",
    (app) => {
      const path = resolve(
        process.cwd(),
        `../../apps/${app}/public/manifest.webmanifest`,
      );
      const manifest = JSON.parse(readFileSync(path, "utf8")) as {
        display?: string;
        lang?: string;
        start_url?: string;
        scope?: string;
      };

      expect(manifest).toMatchObject({
        display: "standalone",
        lang: "ko-KR",
        scope: "/",
      });
      expect(manifest.start_url).toMatch(/^\//);
    },
  );
});
