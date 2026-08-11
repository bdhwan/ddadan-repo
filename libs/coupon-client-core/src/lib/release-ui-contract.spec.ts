import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { describe, expect, it } from "vitest";

const repoRoot = resolve(process.cwd(), "../..");
const read = (path: string) => readFileSync(join(repoRoot, path), "utf8");

describe("WCAG 2.2 AA release contracts", () => {
  const tokens = read("libs/coupon-ui/src/styles/tokens.scss");

  it("keeps every text token pair at or above 4.5:1 in light and dark themes", () => {
    const light = cssVariables(rootBlock(tokens));
    const dark = cssVariables(darkRootBlock(tokens));
    const pairs: Array<[string, string]> = [
      ["--coupon-color-text", "--coupon-color-bg"],
      ["--coupon-color-text", "--coupon-color-surface"],
      ["--coupon-color-text-muted", "--coupon-color-bg"],
      ["--coupon-color-text-muted", "--coupon-color-surface"],
      ["--coupon-color-primary", "--coupon-color-surface"],
      ["--coupon-color-success", "--coupon-color-surface"],
      ["--coupon-color-warning", "--coupon-color-surface"],
      ["--coupon-color-danger", "--coupon-color-surface"],
      ["--coupon-color-on-primary", "--coupon-color-primary"],
    ];

    for (const theme of [light, dark]) {
      for (const [foreground, background] of pairs) {
        expect(
          contrastRatio(theme[foreground], theme[background]),
          `${foreground} on ${background}`,
        ).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  it("provides a visible focus ring, 44px controls, labels and non-color status cues", () => {
    const button = read("libs/coupon-ui/src/lib/button.component.ts");
    const badge = read("libs/coupon-ui/src/lib/badge.component.ts");
    const qr = read("apps/coupon-consumer-app/src/app/my-qr.component.ts");
    const scan = read("apps/coupon-store-app/src/app/store-scan.component.ts");

    expect(tokens).toMatch(/:focus-visible\s*{[^}]*outline:\s*3px/s);
    expect(button).toMatch(/button\s*{[^}]*min-height:\s*44px/s);
    expect(badge).toContain("accessibleLabel()");
    expect(badge).toContain('aria-hidden="true"');
    expect(qr).toContain('role="timer"');
    expect(qr).toContain("'QR 남은 시간 '");
    expect(qr).toContain('window.addEventListener("offline", offline)');
    expect(qr).toContain("연결 후 다시 확인");
    expect(scan).toMatch(/result success[\s\S]*result-icon[\s\S]*거래 ID/);
    expect(scan).toMatch(/result failure[\s\S]*result-icon[\s\S]*요청 식별/);
  });

  it("keeps keyboard focus on wizard steps and restores it after dialogs", () => {
    const campaign = read(
      "apps/coupon-store-app/src/app/campaign-progress.component.ts",
    );
    expect(campaign).toContain("#wizardHeading");
    expect(campaign).toContain("focusAfterRender(this.wizardHeading)");
    expect(campaign).toContain("captureFocus()");
    expect(campaign).toContain("restoreFocus()");
    expect(campaign).toContain('role="dialog"');
    expect(campaign).toContain("document:keydown.escape");
  });

  it("exposes keyboard routes for login, wallet, QR, scan and campaign creation", () => {
    const consumerRoutes = read(
      "apps/coupon-consumer-app/src/app/app.routes.ts",
    );
    const storeRoutes = read("apps/coupon-store-app/src/app/app.routes.ts");
    const consumerShell = read(
      "apps/coupon-consumer-app/src/app/consumer-shell.component.ts",
    );
    const storeShell = read(
      "apps/coupon-store-app/src/app/store-shell.component.ts",
    );
    const campaign = read(
      "apps/coupon-store-app/src/app/campaign-progress.component.ts",
    );

    expect(consumerRoutes).toMatch(/path:\s*"login"/);
    expect(consumerRoutes).toMatch(/path:\s*"wallet"/);
    expect(consumerRoutes).toMatch(/path:\s*"my-qr"/);
    expect(storeRoutes).toMatch(/path:\s*"scan"/);
    expect(storeRoutes).toMatch(/path:\s*"campaigns"/);
    expect(consumerShell).toContain('aria-label="소비자 주요 메뉴"');
    expect(storeShell).toContain('aria-label="상점 주요 메뉴"');
    expect(campaign).toContain("새 캠페인");
  });
});

describe("responsive release contracts", () => {
  it("supports the required 360, 768, 1024 and 1280 breakpoints", () => {
    const consumerGlobal = read("apps/coupon-consumer-app/src/styles.scss");
    const consumerShell = read(
      "apps/coupon-consumer-app/src/app/consumer-shell.component.ts",
    );
    const storeShell = read(
      "apps/coupon-store-app/src/app/store-shell.component.ts",
    );
    const storeFeature = read(
      "apps/coupon-store-app/src/app/store-feature-state.component.ts",
    );
    const adminShell = read(
      "apps/coupon-system-admin-app/src/app/admin-shell.component.ts",
    );

    expect(consumerGlobal).toMatch(/min-width:\s*360px/);
    expect(consumerShell).toMatch(/width:\s*min\(100% - 2rem, 62rem\)/);
    expect(storeShell).toMatch(/@media \(min-width:\s*768px\)/);
    expect(storeFeature).toMatch(/@media \(min-width:\s*1280px\)/);
    expect(adminShell).toMatch(/@media \(max-width:\s*1023px\)/);
    expect(adminShell).toMatch(/@media \(min-width:\s*1024px\)/);
    expect(adminShell).toContain("모바일 읽기 전용");
    expect(adminShell).toContain("guardMobileMutation");
  });
});

function rootBlock(css: string): string {
  return css.match(/^:root\s*{([\s\S]*?)^}/m)?.[1] ?? "";
}

function darkRootBlock(css: string): string {
  return (
    css.match(
      /@media \(prefers-color-scheme: dark\)\s*{\s*:root\s*{([\s\S]*?)\n\s*}\s*}/,
    )?.[1] ?? ""
  );
}

function cssVariables(block: string): Record<string, string> {
  return Object.fromEntries(
    [...block.matchAll(/(--[\w-]+):\s*(#[0-9a-f]{6})\s*;/gi)].map(
      ([, name, value]) => [name, value],
    ),
  );
}

function contrastRatio(foreground: string, background: string): number {
  const lighter = Math.max(luminance(foreground), luminance(background));
  const darker = Math.min(luminance(foreground), luminance(background));
  return (lighter + 0.05) / (darker + 0.05);
}

function luminance(hex: string): number {
  const channels = [1, 3, 5].map(
    (index) => Number.parseInt(hex.slice(index, index + 2), 16) / 255,
  );
  const linear = channels.map((value) =>
    value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4,
  );
  return 0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2];
}
