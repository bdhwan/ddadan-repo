import { Observable, Subject, interval } from 'rxjs';
import { distinctUntilChanged, map, startWith, switchMap } from 'rxjs/operators';

export interface VersionCursor {
  version?: number;
  updated_at?: string;
}

/**
 * Polls immediately and at the requested foreground interval. Hidden tabs stop
 * completely by default; callers can opt into a reduced interval up to 5 min.
 */
export function visibilityAwarePoll(
  foregroundIntervalMs: number,
  hiddenIntervalMs: number | null = null,
  documentRef: Pick<Document, 'visibilityState' | 'addEventListener' | 'removeEventListener'> | null =
    typeof document === 'undefined' ? null : document,
): Observable<void> {
  if (foregroundIntervalMs <= 0 || (hiddenIntervalMs !== null && (hiddenIntervalMs <= 0 || hiddenIntervalMs > 300_000))) {
    throw new RangeError('polling intervals must be positive and hidden polling cannot exceed 5 minutes');
  }
  if (!documentRef) return interval(foregroundIntervalMs).pipe(startWith(0), map(() => undefined));

  const visibility$ = new Observable<DocumentVisibilityState>((subscriber) => {
    const emit = () => subscriber.next(documentRef.visibilityState);
    documentRef.addEventListener('visibilitychange', emit);
    emit();
    return () => documentRef.removeEventListener('visibilitychange', emit);
  });

  return visibility$.pipe(
    distinctUntilChanged(),
    switchMap((state) => {
      if (state === 'hidden' && hiddenIntervalMs === null) return new Subject<void>();
      const delay = state === 'hidden' ? hiddenIntervalMs! : foregroundIntervalMs;
      return interval(delay).pipe(startWith(0), map(() => undefined));
    }),
  );
}

export function versionQuery(cursor: VersionCursor | null): Record<string, string> {
  if (!cursor) return {};
  return {
    ...(cursor.version === undefined ? {} : { version: String(cursor.version) }),
    ...(cursor.updated_at ? { updated_at: cursor.updated_at } : {}),
  };
}
