import '@angular/compiler';
import { HttpErrorResponse, HttpHeaders, HttpRequest, HttpResponse } from '@angular/common/http';
import { firstValueFrom, of, throwError } from 'rxjs';
import { describe, expect, it, vi } from 'vitest';
import { CouponErrorMapper } from './client-error';
import { interceptCouponRequest, type CouponTokenProvider } from './http.interceptor';

describe('coupon auth interceptor', () => {
  it('refreshes the token once and retries the original request only once', async () => {
    const tokenCalls: boolean[] = [];
    const auth: CouponTokenProvider = {
      currentUser: {},
      async getIdToken(forceRefresh = false) {
        tokenCalls.push(forceRefresh);
        return forceRefresh ? 'fresh-token' : 'stale-token';
      },
    };
    const seen: HttpRequest<unknown>[] = [];
    const next = vi.fn((request: HttpRequest<unknown>) => {
      seen.push(request);
      return seen.length === 1
        ? throwError(() => new HttpErrorResponse({ status: 401 }))
        : of(new HttpResponse({ status: 200, body: { request_id: 'req-ok' } }));
    });

    await firstValueFrom(
      interceptCouponRequest(
        new HttpRequest('POST', '/api/coupon/v1/me/qr-tokens', {}),
        next,
        auth,
        new CouponErrorMapper(),
        { recordRequest: vi.fn() },
      ),
    );

    expect(tokenCalls).toEqual([false, true]);
    expect(next).toHaveBeenCalledTimes(2);
    expect(seen[0].headers.get('Authorization')).toBe('Bearer stale-token');
    expect(seen[1].headers.get('Authorization')).toBe('Bearer fresh-token');
    expect(seen[1].headers.get('Idempotency-Key')).toBe(seen[0].headers.get('Idempotency-Key'));
  });

  it('does not enter an infinite refresh loop when the retried request is also unauthorized', async () => {
    const auth: CouponTokenProvider = {
      currentUser: {},
      getIdToken: vi.fn(async (forceRefresh = false) => (forceRefresh ? 'fresh' : 'stale')),
    };
    const next = vi.fn(() => throwError(() => new HttpErrorResponse({
      status: 401,
      error: { error: { code: 'AUTH_EXPIRED', message: 'expired', field_errors: [], retryable: false, request_id: 'req-401' } },
    })));

    await expect(firstValueFrom(interceptCouponRequest(
      new HttpRequest('GET', '/api/coupon/v1/me'),
      next,
      auth,
      new CouponErrorMapper(),
      { recordRequest: vi.fn() },
    ))).rejects.toMatchObject({ code: 'AUTH_EXPIRED', status: 401 });

    expect(auth.getIdToken).toHaveBeenCalledTimes(2);
    expect(next).toHaveBeenCalledTimes(2);
  });

  it('preserves an explicitly supplied idempotency key', async () => {
    const next = vi.fn((request: HttpRequest<unknown>) => {
      expect(request.headers.get('Idempotency-Key')).toBe('4ccf0150-7a27-4fb8-9c14-8ce268261f2d');
      return of(new HttpResponse({ status: 200 }));
    });
    await firstValueFrom(interceptCouponRequest(
      new HttpRequest('POST', '/mutation', {}, {
        headers: new HttpHeaders({ 'Idempotency-Key': '4ccf0150-7a27-4fb8-9c14-8ce268261f2d' }),
      }),
      next,
      { currentUser: null, getIdToken: async () => null },
      new CouponErrorMapper(),
      { recordRequest: vi.fn() },
    ));
  });
});
