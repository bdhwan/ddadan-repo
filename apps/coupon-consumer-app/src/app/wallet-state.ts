import type { WalletCouponDto, WalletStampBoardDto } from '@coupon/contracts';

export type WalletViewStatus = 'loading' | 'ready' | 'empty' | 'stale' | 'error' | 'offline';

export interface WalletSnapshot {
  coupons: WalletCouponDto[];
  stamps: WalletStampBoardDto[];
  synced_at: string | null;
  version: number | null;
  updated_at: string | null;
}

export interface WalletViewState extends WalletSnapshot {
  status: WalletViewStatus;
  message: string | null;
  read_only: boolean;
}

export type WalletEvent =
  | { type: 'LOAD' }
  | { type: 'SUCCESS'; snapshot: WalletSnapshot }
  | { type: 'FAILURE'; message: string; cached: WalletSnapshot | null; online: boolean }
  | { type: 'OFFLINE'; cached: WalletSnapshot | null }
  | { type: 'STALE'; snapshot: WalletSnapshot };

export const EMPTY_WALLET_SNAPSHOT: WalletSnapshot = {
  coupons: [], stamps: [], synced_at: null, version: null, updated_at: null,
};

export function reduceWalletState(state: WalletViewState, event: WalletEvent): WalletViewState {
  switch (event.type) {
    case 'LOAD':
      return { ...state, status: 'loading', message: null };
    case 'SUCCESS':
      return {
        ...event.snapshot,
        status: event.snapshot.coupons.length || event.snapshot.stamps.length ? 'ready' : 'empty',
        message: null,
        read_only: false,
      };
    case 'STALE':
      return { ...event.snapshot, status: 'stale', message: '서버의 최신 버전을 확인하는 중입니다.', read_only: true };
    case 'OFFLINE':
      return event.cached
        ? { ...event.cached, status: 'offline', message: '오프라인입니다. 마지막 동기화 내역만 볼 수 있습니다.', read_only: true }
        : { ...EMPTY_WALLET_SNAPSHOT, status: 'offline', message: '온라인 연결 후 지갑을 불러올 수 있습니다.', read_only: true };
    case 'FAILURE':
      if (event.cached) {
        return {
          ...event.cached,
          status: event.online ? 'stale' : 'offline',
          message: event.online ? '최신 정보를 가져오지 못해 마지막 동기화 내역을 표시합니다.' : '오프라인입니다. 마지막 동기화 내역만 볼 수 있습니다.',
          read_only: true,
        };
      }
      return { ...EMPTY_WALLET_SNAPSHOT, status: event.online ? 'error' : 'offline', message: event.message, read_only: true };
    default:
      return assertNever(event);
  }
}

export function initialWalletState(): WalletViewState {
  return { ...EMPTY_WALLET_SNAPSHOT, status: 'loading', message: null, read_only: false };
}

function assertNever(value: never): never {
  throw new Error(`Unknown wallet event: ${JSON.stringify(value)}`);
}
