import { NgTemplateOutlet } from '@angular/common';
import { HttpClient } from '@angular/common/http';
import { Component, computed, inject, OnDestroy, OnInit, signal } from '@angular/core';
import { environment } from '../environment';

interface Badge {
  text: string;
  variant?: 'best' | 'rec' | 'info' | 'warn';
}

interface ScreenItem {
  id: string;
  kind: 'image' | 'video' | 'text';
  url?: string;
  text?: string;
  fontSize?: number;
  fontUnit?: 'px' | 'vh';
  color?: string;
  background?: string;
  opacity?: number;
  fontWeight?: number;
  textAlign?: 'left' | 'center' | 'right';
  lineHeight?: number;
  textVariant?: 'plain' | 'menuLine' | 'groupHeader' | 'note';
  textSecondary?: string;
  textEn?: string;
  priceExtra?: string;
  priceColor?: string;
  badges?: Badge[];
  x: number;
  y: number;
  width: number;
  height: number;
  zIndex?: number;
  radius?: number;
}

interface SlidePayload {
  width: number;
  height: number;
  background?: string;
  items: ScreenItem[];
}

interface ScreenResponse {
  registered: boolean;
  deviceName: string | null;
  mode?: 'single' | 'rotation';
  width: number;
  height: number;
  background?: string;
  items: ScreenItem[];
  rotation?: {
    intervalMs: number;
    fadeMs: number;
    slides: SlidePayload[];
  };
  isFallback?: boolean;
}

@Component({
  selector: 'app-root',
  imports: [NgTemplateOutlet],
  templateUrl: './app.html',
  styleUrl: './app.scss',
})
export class App implements OnInit, OnDestroy {
  private readonly http = inject(HttpClient);
  protected readonly screen = signal<ScreenResponse | null>(null);
  protected readonly hardwareId = signal('');
  protected readonly slot = signal(0);

  /**
   * 유효 API 베이스. 안드로이드 박스는 이 SPA를 file://로 번들 로드하므로
   * 발견된 서버 주소를 `?api=` 쿼리로 주입한다(없으면 빌드 환경값).
   */
  private apiBaseUrl = environment.apiBase;
  /** 풀스크린 키오스크(박스 WebView)에서 레터박스(aspect-ratio) 없이 화면을 꽉 채운다. */
  protected readonly fillMode = signal(false);
  protected readonly aspect = computed(() => {
    const s = this.screen();
    return s ? `${s.width} / ${s.height}` : '16 / 9';
  });

  /** 크로스 페이드 상·하단 슬라이드 인덱스 */
  protected readonly rotIdx0 = signal(0);
  protected readonly rotIdx1 = signal(1);
  protected readonly rotOp0 = signal(1);
  protected readonly rotOp1 = signal(0);
  protected readonly rotTransition = signal('none');
  protected readonly rotationSlides = signal<SlidePayload[] | null>(null);

  protected readonly aspectRot = computed(() => {
    const slides = this.rotationSlides();
    const s0 = slides?.[0];
    return s0 ? `${s0.width} / ${s0.height}` : '16 / 9';
  });

  protected readonly useRotation = computed(() => (this.rotationSlides()?.length ?? 0) >= 2);

  private timer: ReturnType<typeof setInterval> | null = null;
  private lastRotationKey = '';
  private rotWaitTimer: ReturnType<typeof setTimeout> | null = null;
  private rotFadeTimer: ReturnType<typeof setTimeout> | null = null;

  protected readonly editingDeviceId = signal(false);
  protected readonly draftDeviceId = signal('');
  protected readonly needsRegister = computed(() => {
    const s = this.screen();
    return s !== null && (s.registered === false || s.isFallback === true);
  });

  ngOnInit(): void {
    const url = new URL(window.location.href);
    const apiParam = url.searchParams.get('api');
    if (apiParam) this.apiBaseUrl = apiParam;
    this.fillMode.set(url.searchParams.get('fill') === '1');
    const queryDeviceId = url.searchParams.get('deviceId') ?? url.searchParams.get('device');
    let stored: string | null = null;
    try {
      stored = localStorage.getItem('ddadan.deviceId');
    } catch {
      // ignore (private mode, etc.)
    }
    const deviceId = queryDeviceId ?? stored ?? 'dev-local';
    const slot = Number(url.searchParams.get('slot') ?? '0');
    this.hardwareId.set(deviceId);
    this.slot.set(slot);

    this.fetch();
    this.timer = setInterval(() => this.fetch(), environment.pollIntervalMs);
  }

  protected openDeviceIdEditor() {
    this.draftDeviceId.set(this.hardwareId());
    this.editingDeviceId.set(true);
  }

  protected cancelDeviceIdEditor() {
    this.editingDeviceId.set(false);
  }

  protected applyDeviceId(value: string) {
    const next = value.trim();
    if (!next) return;
    try {
      localStorage.setItem('ddadan.deviceId', next);
    } catch {
      // ignore
    }
    this.hardwareId.set(next);
    this.editingDeviceId.set(false);
    this.fetch();
  }

  ngOnDestroy(): void {
    if (this.timer) clearInterval(this.timer);
    this.clearRotationTimers();
  }

  private clearRotationTimers() {
    if (this.rotWaitTimer) clearTimeout(this.rotWaitTimer);
    if (this.rotFadeTimer) clearTimeout(this.rotFadeTimer);
    this.rotWaitTimer = null;
    this.rotFadeTimer = null;
  }

  protected isMenuLine(item: ScreenItem): boolean {
    return item.textVariant === 'menuLine';
  }

  protected isGroupHeader(item: ScreenItem): boolean {
    return item.textVariant === 'groupHeader';
  }

  protected isNote(item: ScreenItem): boolean {
    return item.textVariant === 'note';
  }

  /** 라벨 앞에 붙는 원형 뱃지(BEST/추천). */
  protected prefixBadges(item: ScreenItem): Badge[] {
    return (item.badges ?? []).filter((b) => b.variant === 'best' || b.variant === 'rec');
  }

  /** 라벨 뒤(가격 앞)에 붙는 인라인 태그(ICED Only/DECAF 등). */
  protected suffixBadges(item: ScreenItem): Badge[] {
    return (item.badges ?? []).filter((b) => b.variant !== 'best' && b.variant !== 'rec');
  }

  protected textAlign(item: ScreenItem): string {
    if (item.textAlign) return item.textAlign;
    return this.isMenuLine(item) || this.isGroupHeader(item) ? 'left' : 'center';
  }

  protected fontCss(item: ScreenItem): string | null {
    if (item.fontSize == null) return null;
    return `${item.fontSize}${item.fontUnit ?? 'px'}`;
  }

  protected absoluteUrl(item: ScreenItem): string | null {
    if (!item.url) return null;
    if (item.url.startsWith('http')) return item.url;
    return `${this.apiBaseUrl.replace(/\/api$/, '')}${item.url}`;
  }

  private fetch() {
    const hwid = this.hardwareId();
    if (!hwid) return;
    this.http
      .get<ScreenResponse>(`${this.apiBaseUrl}/player/${encodeURIComponent(hwid)}/screen?slot=${this.slot()}`)
      .subscribe({
        next: (res) => this.applyResponse(res),
        error: (err) => console.warn('player fetch failed', err.message),
      });
  }

  private applyResponse(res: ScreenResponse) {
    this.screen.set(res);
    const slides = res.rotation?.slides;
    if (res.mode === 'rotation' && slides && slides.length >= 2) {
      const key = `${slides.length}|${res.rotation!.intervalMs}|${res.rotation!.fadeMs}|${slides.map((s) => s.items.map((i) => i.id).join(',')).join('||')}`;
      if (key !== this.lastRotationKey) {
        this.lastRotationKey = key;
        this.clearRotationTimers();
        this.rotationSlides.set(slides);
        this.rotIdx0.set(0);
        this.rotIdx1.set(1);
        this.rotOp0.set(1);
        this.rotOp1.set(0);
        this.rotTransition.set('none');
        this.scheduleRotationStep(res.rotation!.intervalMs, res.rotation!.fadeMs);
      }
    } else {
      this.lastRotationKey = '';
      this.clearRotationTimers();
      this.rotationSlides.set(null);
    }
  }

  private scheduleRotationStep(intervalMs: number, fadeMs: number) {
    this.rotWaitTimer = setTimeout(() => {
      this.rotTransition.set(`opacity ${fadeMs}ms linear`);
      this.rotOp0.set(0);
      this.rotOp1.set(1);
      this.rotFadeTimer = setTimeout(() => {
        const slides = this.rotationSlides();
        if (!slides?.length) return;
        const n = slides.length;
        const nextTop = (this.rotIdx1() + 1) % n;
        this.rotIdx0.set(this.rotIdx1());
        this.rotIdx1.set(nextTop);
        this.rotTransition.set('none');
        this.rotOp0.set(1);
        this.rotOp1.set(0);
        this.scheduleRotationStep(intervalMs, fadeMs);
      }, fadeMs);
    }, intervalMs);
  }

  protected slideAt(which: 0 | 1): SlidePayload | null {
    const slides = this.rotationSlides();
    if (!slides?.length) return null;
    const idx = which === 0 ? this.rotIdx0() : this.rotIdx1();
    return slides[idx] ?? null;
  }
}
