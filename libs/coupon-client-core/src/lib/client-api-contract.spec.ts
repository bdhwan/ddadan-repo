import { readFileSync, readdirSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { describe, expect, it } from "vitest";

interface ClientOperation {
  source: string;
  method: "get" | "post" | "put" | "patch" | "delete";
  path: string;
  screen: string;
}

const CLIENT_OPERATIONS: readonly ClientOperation[] = [
  {
    source: "account.api.ts",
    method: "get",
    path: "/api/coupon/v1/me/consents",
    screen: "소비자 알림 설정",
  },
  {
    source: "account.api.ts",
    method: "post",
    path: "/api/coupon/v1/me/consents",
    screen: "소비자 알림 설정",
  },
  {
    source: "notifications.api.ts",
    method: "get",
    path: "/api/coupon/v1/me/notifications",
    screen: "소비자 알림",
  },
  {
    source: "notifications.api.ts",
    method: "patch",
    path: "/api/coupon/v1/me/notifications",
    screen: "소비자 알림",
  },
  {
    source: "wallet.api.ts",
    method: "get",
    path: "/api/coupon/v1/me/wallet/coupons",
    screen: "소비자 지갑",
  },
  {
    source: "wallet.api.ts",
    method: "get",
    path: "/api/coupon/v1/me/wallet/stamps",
    screen: "소비자 지갑",
  },
  {
    source: "my-qr.api.ts",
    method: "post",
    path: "/api/coupon/v1/me/qr-tokens",
    screen: "소비자 내 QR",
  },
  {
    source: "store-detail.api.ts",
    method: "post",
    path: "/api/coupon/v1/campaigns/{campaign_id}/claims",
    screen: "공개 캠페인 받기",
  },
  {
    source: "store-onboarding.api.ts",
    method: "get",
    path: "/api/coupon/v1/owner/store",
    screen: "점주 온보딩",
  },
  {
    source: "store-onboarding.api.ts",
    method: "post",
    path: "/api/coupon/v1/owner/store",
    screen: "점주 온보딩",
  },
  {
    source: "store-onboarding.api.ts",
    method: "patch",
    path: "/api/coupon/v1/owner/store",
    screen: "점주 온보딩",
  },
  {
    source: "store-onboarding.api.ts",
    method: "post",
    path: "/api/coupon/v1/owner/store/submit-review",
    screen: "점주 온보딩",
  },
  {
    source: "catalog.api.ts",
    method: "get",
    path: "/api/coupon/v1/owner/catalog/items",
    screen: "점주 품목",
  },
  {
    source: "catalog.api.ts",
    method: "post",
    path: "/api/coupon/v1/owner/catalog/items",
    screen: "점주 품목",
  },
  {
    source: "catalog.api.ts",
    method: "patch",
    path: "/api/coupon/v1/owner/catalog/items/{item_id}",
    screen: "점주 품목",
  },
  {
    source: "loyalty.api.ts",
    method: "get",
    path: "/api/coupon/v1/owner/loyalty-policies",
    screen: "점주 도장 정책",
  },
  {
    source: "loyalty.api.ts",
    method: "post",
    path: "/api/coupon/v1/owner/loyalty-policies",
    screen: "점주 도장 정책",
  },
  {
    source: "loyalty.api.ts",
    method: "patch",
    path: "/api/coupon/v1/owner/loyalty-policies/{policy_id}",
    screen: "점주 도장 정책",
  },
  {
    source: "loyalty.api.ts",
    method: "post",
    path: "/api/coupon/v1/owner/loyalty-policies/{policy_id}/publish",
    screen: "점주 도장 정책",
  },
  {
    source: "campaigns.api.ts",
    method: "get",
    path: "/api/coupon/v1/owner/campaigns",
    screen: "점주 캠페인",
  },
  {
    source: "campaigns.api.ts",
    method: "post",
    path: "/api/coupon/v1/owner/campaigns",
    screen: "점주 캠페인",
  },
  {
    source: "campaigns.api.ts",
    method: "patch",
    path: "/api/coupon/v1/owner/campaigns/{campaign_id}",
    screen: "점주 캠페인",
  },
  {
    source: "campaigns.api.ts",
    method: "post",
    path: "/api/coupon/v1/owner/campaigns/{campaign_id}/publish",
    screen: "점주 캠페인",
  },
  {
    source: "campaigns.api.ts",
    method: "post",
    path: "/api/coupon/v1/owner/campaigns/{campaign_id}/pause",
    screen: "점주 캠페인",
  },
  {
    source: "campaigns.api.ts",
    method: "post",
    path: "/api/coupon/v1/owner/campaigns/{campaign_id}/resume",
    screen: "점주 캠페인",
  },
  {
    source: "campaigns.api.ts",
    method: "post",
    path: "/api/coupon/v1/owner/campaigns/{campaign_id}/cancel",
    screen: "점주 캠페인",
  },
  {
    source: "store-operations.api.ts",
    method: "post",
    path: "/api/coupon/v1/owner/scan/resolve",
    screen: "점주 스캔",
  },
  {
    source: "store-operations.api.ts",
    method: "post",
    path: "/api/coupon/v1/owner/stamp-transactions/preview",
    screen: "점주 스캔",
  },
  {
    source: "store-operations.api.ts",
    method: "post",
    path: "/api/coupon/v1/owner/stamp-transactions",
    screen: "점주 스캔",
  },
  {
    source: "store-operations.api.ts",
    method: "post",
    path: "/api/coupon/v1/owner/redemptions/preview",
    screen: "점주 스캔",
  },
  {
    source: "store-operations.api.ts",
    method: "post",
    path: "/api/coupon/v1/owner/redemptions/{reservation_id}/confirm",
    screen: "점주 스캔",
  },
  {
    source: "store-operations.api.ts",
    method: "post",
    path: "/api/coupon/v1/owner/redemptions/{reservation_id}/cancel",
    screen: "점주 스캔",
  },
  {
    source: "store-operations.api.ts",
    method: "get",
    path: "/api/coupon/v1/owner/analytics",
    screen: "점주 오늘",
  },
  {
    source: "analytics.api.ts",
    method: "get",
    path: "/api/coupon/v1/owner/analytics",
    screen: "점주 통계",
  },
  {
    source: "admin-transactions.api.ts",
    method: "get",
    path: "/api/coupon/v1/admin/transactions/{transaction_id}",
    screen: "관리자 거래 탐색",
  },
  {
    source: "admin-operations.api.ts",
    method: "post",
    path: "/api/coupon/v1/admin/campaigns/{campaign_id}/emergency-stop",
    screen: "관리자 캠페인",
  },
  {
    source: "admin-operations.api.ts",
    method: "post",
    path: "/api/coupon/v1/admin/campaigns/{campaign_id}/revoke-job",
    screen: "관리자 캠페인",
  },
  {
    source: "admin-operations.api.ts",
    method: "get",
    path: "/api/coupon/v1/admin/jobs",
    screen: "관리자 작업 큐",
  },
  {
    source: "admin-operations.api.ts",
    method: "post",
    path: "/api/coupon/v1/admin/jobs/{job_id}/retry",
    screen: "관리자 작업 큐",
  },
  {
    source: "admin-phase-four.api.ts",
    method: "get",
    path: "/api/coupon/v1/admin/metrics",
    screen: "관리자 운영·알림",
  },
  {
    source: "admin-phase-four.api.ts",
    method: "get",
    path: "/api/coupon/v1/admin/store-reviews",
    screen: "관리자 상점 검수",
  },
  {
    source: "admin-phase-four.api.ts",
    method: "post",
    path: "/api/coupon/v1/admin/store-reviews/{review_id}/decision",
    screen: "관리자 상점 검수",
  },
  {
    source: "admin-phase-four.api.ts",
    method: "get",
    path: "/api/coupon/v1/admin/cases",
    screen: "관리자 민원",
  },
  {
    source: "admin-phase-four.api.ts",
    method: "get",
    path: "/api/coupon/v1/admin/audit-logs",
    screen: "관리자 감사",
  },
  {
    source: "admin-phase-four.api.ts",
    method: "post",
    path: "/api/coupon/v1/admin/users/{user_id}/revoke-sessions",
    screen: "관리자 회원·상점",
  },
  {
    source: "admin-phase-four.api.ts",
    method: "post",
    path: "/api/coupon/v1/admin/users/{user_id}/suspend",
    screen: "관리자 회원·상점",
  },
];

const repoRoot = resolve(process.cwd(), "../..");
const apiDirectories = [
  "apps/coupon-consumer-app/src/app",
  "apps/coupon-store-app/src/app",
  "apps/coupon-system-admin-app/src/app",
];

describe("client API to OpenAPI release contract", () => {
  const openapi = JSON.parse(
    readFileSync(join(repoRoot, "apps/coupon-api-server/openapi.json"), "utf8"),
  ) as { paths: Record<string, Record<string, unknown>> };

  it.each(CLIENT_OPERATIONS)(
    "$screen: $method $path is implemented by the server",
    ({ method, path }) => {
      expect(openapi.paths[path]?.[method]).toBeDefined();
    },
  );

  it("inventories every client API source file", () => {
    const actualSources = apiDirectories.flatMap((directory) =>
      readdirSync(join(repoRoot, directory))
        .filter((file) => file.endsWith(".api.ts"))
        .filter((file) =>
          readFileSync(join(repoRoot, directory, file), "utf8").includes(
            "HttpClient",
          ),
        ),
    );
    const inventoriedSources = [
      ...new Set(CLIENT_OPERATIONS.map((item) => item.source)),
    ];
    expect(actualSources.sort()).toEqual(inventoriedSources.sort());
  });

  it("contains no call to an unimplemented or invented path", () => {
    const apiSource = apiDirectories
      .flatMap((directory) =>
        readdirSync(join(repoRoot, directory))
          .filter((file) => file.endsWith(".api.ts"))
          .map((file) => readFileSync(join(repoRoot, directory, file), "utf8")),
      )
      .join("\n");

    expect(apiSource).not.toMatch(
      /\/admin\/(members|notifications)(?:[\/`"']|$)|\/admin\/campaigns[`"']|\/me\/sessions|\/me\/withdrawal|\/public\/stores|\/me\/favorite-stores/,
    );
  });
});
