import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import {
  type CouponClientRuntimeOptions,
  resolveAuthEmulatorUrl,
} from "./firebase-auth";

describe("Firebase Auth emulator configuration", () => {
  it("uses the HTTPS application origin so LAN auth has no mixed content", () => {
    const options: CouponClientRuntimeOptions = {
      production: false,
      authEmulator: { enabled: true, useSameOrigin: true },
    };

    expect(
      resolveAuthEmulatorUrl(options, "https://192.168.150.185:4310/login"),
    ).toBe("https://192.168.150.185:4310");
  });

  it("does not configure an emulator when it is disabled", () => {
    expect(
      resolveAuthEmulatorUrl({
        production: true,
        authEmulator: { enabled: false, useSameOrigin: true },
      }),
    ).toBeNull();
  });

  it("refuses an enabled emulator in every production configuration", () => {
    expect(() =>
      resolveAuthEmulatorUrl(
        {
          production: true,
          authEmulator: { enabled: true, useSameOrigin: true },
        },
        "https://coupon.example.com",
      ),
    ).toThrowError("AUTH_EMULATOR_FORBIDDEN_IN_PRODUCTION");
  });

  it.each([
    "coupon-consumer-app",
    "coupon-store-app",
    "coupon-system-admin-app",
  ])(
    "keeps emulator settings out of the %s production build",
    (application) => {
      const applicationRoot = resolve(
        process.cwd(),
        `../../apps/${application}`,
      );
      const angularConfig = readFileSync(
        resolve(applicationRoot, "angular.json"),
        "utf8",
      );
      const productionEnvironment = readFileSync(
        resolve(applicationRoot, "src/environments/environment.prod.ts"),
        "utf8",
      );

      expect(angularConfig).toContain(
        '"with": "src/environments/environment.prod.ts"',
      );
      expect(productionEnvironment).toMatch(/production:\s*true/);
      expect(productionEnvironment).toMatch(
        /authEmulator:\s*{\s*enabled:\s*false/,
      );
      expect(productionEnvironment).not.toMatch(
        /ddadan-dev|192\.168\.150\.185|9099/,
      );
    },
  );
});
