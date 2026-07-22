import { bootstrapApplication } from '@angular/platform-browser';
import { appConfig } from './app/app.config';
import { App } from './app/app';

// ── 구형 브라우저/WebView(Chromium 72 등) 호환 shim ────────────────────────
// Angular 19는 PerformanceObserver.observe({type: ...}) (단일 type, Chrome 78+)를
// 호출하는데 구형 엔진은 {entryTypes: [...]} 형식만 지원한다. {type}을 변환하고, 그래도
// 미지원 entryType이면 조용히 무시한다(사이니지 플레이어는 성능 관측이 불필요).
(function patchPerformanceObserver() {
  const PO = (window as unknown as { PerformanceObserver?: { prototype: { observe: (o: unknown) => void } } }).PerformanceObserver;
  if (!PO || !PO.prototype || !PO.prototype.observe) return;
  const orig = PO.prototype.observe;
  PO.prototype.observe = function (this: unknown, options: { type?: string; entryTypes?: string[]; buffered?: boolean }) {
    try {
      let opts: unknown = options;
      if (options && options.type && !options.entryTypes) {
        opts = { entryTypes: [options.type], buffered: options.buffered };
      }
      return orig.call(this, opts);
    } catch {
      return undefined;
    }
  } as typeof orig;
})();
// ─────────────────────────────────────────────────────────────────────────

bootstrapApplication(App, appConfig)
  .catch((err) => console.error(err));
