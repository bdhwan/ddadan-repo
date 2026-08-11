import {
  HttpContextToken,
  HttpErrorResponse,
  HttpEvent,
  HttpEventType,
  HttpHandlerFn,
  HttpInterceptorFn,
  HttpRequest,
} from '@angular/common/http';
import { inject } from '@angular/core';
import { catchError, from, Observable, switchMap, tap, throwError } from 'rxjs';
import { AuthSessionService } from './firebase-auth';
import { CouponErrorMapper } from './client-error';
import { CouponTelemetryService } from './telemetry';

const TOKEN_REFRESH_ATTEMPTED = new HttpContextToken<boolean>(() => false);
const MUTATION_METHODS = new Set(['POST', 'PUT', 'PATCH', 'DELETE']);

export const couponHttpInterceptor: HttpInterceptorFn = (request, next) => {
  const auth = inject(AuthSessionService);
  const errors = inject(CouponErrorMapper);
  const telemetry = inject(CouponTelemetryService);

  return interceptCouponRequest(request, next, auth, errors, telemetry);
};

export interface CouponTokenProvider {
  readonly currentUser: unknown | null;
  getIdToken(forceRefresh?: boolean): Promise<string | null>;
}

/** Exported for transport-level tests without booting Firebase. */
export function interceptCouponRequest(
  request: HttpRequest<unknown>,
  next: HttpHandlerFn,
  auth: CouponTokenProvider,
  errors: Pick<CouponErrorMapper, 'from'>,
  telemetry: Pick<CouponTelemetryService, 'recordRequest'>,
): Observable<HttpEvent<unknown>> {
  let preparedRequest = request;
  return from(auth.getIdToken()).pipe(
    switchMap((token) => {
      preparedRequest = prepareRequest(request, token);
      return next(preparedRequest);
    }),
    tap((event) => recordResponse(event, request, telemetry)),
    catchError((error: unknown) => {
      recordErrorResponse(error, request, telemetry);
      if (
        error instanceof HttpErrorResponse &&
        error.status === 401 &&
        !request.context.get(TOKEN_REFRESH_ATTEMPTED) &&
        auth.currentUser
      ) {
        // The context flag travels with the single retry. Calling `next`
        // avoids restarting this interceptor chain, preventing refresh loops.
        return from(auth.getIdToken(true)).pipe(
          switchMap((token) => {
            const retry = prepareRequest(
              preparedRequest.clone({
                headers: preparedRequest.headers.delete('Authorization'),
                context: request.context.set(TOKEN_REFRESH_ATTEMPTED, true),
              }),
              token,
            );
            return next(retry);
          }),
          tap((event) => recordResponse(event, request, telemetry)),
          catchError((retryError: unknown) => {
            recordErrorResponse(retryError, request, telemetry);
            return throwError(() => errors.from(retryError));
          }),
        );
      }
      return throwError(() => errors.from(error));
    }),
  );
}

function prepareRequest(request: HttpRequest<unknown>, idToken: string | null): HttpRequest<unknown> {
  let headers = request.headers;
  if (idToken && !headers.has('Authorization')) {
    headers = headers.set('Authorization', `Bearer ${idToken}`);
  }
  if (MUTATION_METHODS.has(request.method) && !headers.has('Idempotency-Key')) {
    headers = headers.set('Idempotency-Key', createUuid());
  }
  return request.clone({ headers });
}

function recordErrorResponse(
  error: unknown,
  request: HttpRequest<unknown>,
  telemetry: Pick<CouponTelemetryService, 'recordRequest'>,
): void {
  if (!(error instanceof HttpErrorResponse) || typeof error.error !== 'object' || error.error === null) {
    return;
  }
  const requestId = (error.error as { error?: { request_id?: unknown } }).error?.request_id;
  if (typeof requestId === 'string') {
    telemetry.recordRequest({
      request_id: requestId,
      method: request.method,
      url: request.urlWithParams,
      status: error.status,
    });
  }
}

function recordResponse(
  event: HttpEvent<unknown>,
  request: HttpRequest<unknown>,
  telemetry: Pick<CouponTelemetryService, 'recordRequest'>,
): void {
  if (event.type !== HttpEventType.Response || typeof event.body !== 'object' || event.body === null) {
    return;
  }
  const requestId = (event.body as { request_id?: unknown }).request_id;
  if (typeof requestId === 'string') {
    telemetry.recordRequest({
      request_id: requestId,
      method: request.method,
      url: request.urlWithParams,
      status: event.status,
    });
  }
}

function createUuid(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (character) => {
    const random = Math.floor(Math.random() * 16);
    const value = character === 'x' ? random : (random & 0x3) | 0x8;
    return value.toString(16);
  });
}
