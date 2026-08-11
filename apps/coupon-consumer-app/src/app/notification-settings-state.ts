import type {
  BrowserPermissionState,
  ConsentScopeDto,
  ConsentStateDto,
} from "@coupon/contracts";

export function currentBrowserPermission(): BrowserPermissionState {
  return typeof Notification === "undefined"
    ? "unsupported"
    : Notification.permission;
}

export function permissionCopy(permission: BrowserPermissionState): string {
  return {
    granted: "이 브라우저에서 푸시 알림이 허용돼 있습니다.",
    denied:
      "푸시 알림이 차단됐습니다. 브라우저 사이트 설정에서 권한을 복구해 주세요.",
    default: "푸시 알림을 받으려면 브라우저 권한이 필요합니다.",
    unsupported: "이 브라우저는 Web Push 권한 요청을 지원하지 않습니다.",
  }[permission];
}

export function optimisticConsent(
  consents: readonly ConsentStateDto[],
  scope: ConsentScopeDto,
  storeId: string | null,
  granted: boolean,
  decidedAt: string,
): ConsentStateDto[] {
  return consents.map((consent) =>
    consent.scope === scope && consent.store_id === storeId
      ? { ...consent, granted, decided_at: decidedAt }
      : consent,
  );
}
