import { Component, computed, inject, OnDestroy, OnInit, signal } from '@angular/core';
import { ActivatedRoute } from '@angular/router';
import {
  ApiService,
  PlayerScreenItem,
  PlayerScreenResponse,
  PlayerSlidePayload,
} from '../api.service';

@Component({
  standalone: true,
  templateUrl: './device-preview.page.html',
  styleUrl: './device-preview.page.scss',
})
export class DevicePreviewPage implements OnInit, OnDestroy {
  private readonly api = inject(ApiService);
  private readonly route = inject(ActivatedRoute);

  readonly hardwareId = signal('');
  readonly slot = signal(0);
  readonly screen = signal<PlayerScreenResponse | null>(null);
  readonly loadError = signal<string | null>(null);

  readonly aspect = computed(() => {
    const s = this.screen();
    return s ? `${s.width} / ${s.height}` : '16 / 9';
  });

  readonly rotIdx0 = signal(0);
  readonly rotIdx1 = signal(1);
  readonly rotOp0 = signal(1);
  readonly rotOp1 = signal(0);
  readonly rotTransition = signal('none');
  readonly rotationSlides = signal<PlayerSlidePayload[] | null>(null);

  readonly aspectRot = computed(() => {
    const slides = this.rotationSlides();
    const s0 = slides?.[0];
    return s0 ? `${s0.width} / ${s0.height}` : '16 / 9';
  });

  readonly useRotation = computed(() => (this.rotationSlides()?.length ?? 0) >= 2);

  private pollTimer: ReturnType<typeof setInterval> | null = null;
  private lastRotationKey = '';
  private rotWaitTimer: ReturnType<typeof setTimeout> | null = null;
  private rotFadeTimer: ReturnType<typeof setTimeout> | null = null;

  ngOnInit() {
    this.route.paramMap.subscribe((params) => {
      this.hardwareId.set(params.get('hardwareId') ?? '');
      this.fetch();
    });
    this.route.queryParamMap.subscribe((params) => {
      this.slot.set(Number(params.get('slot') ?? '0'));
      this.fetch();
    });
    this.pollTimer = setInterval(() => this.fetch(), 10_000);
  }

  ngOnDestroy() {
    if (this.pollTimer) clearInterval(this.pollTimer);
    this.clearRotationTimers();
  }

  isMenuLine(item: PlayerScreenItem): boolean {
    return item.textVariant === 'menuLine';
  }

  textAlign(item: PlayerScreenItem): string {
    return item.textAlign ?? (this.isMenuLine(item) ? 'left' : 'center');
  }

  absoluteUrl(item: PlayerScreenItem): string | null {
    if (!item.url) return null;
    return this.api.absoluteAssetUrl(item.url);
  }

  slideAt(which: 0 | 1): PlayerSlidePayload | null {
    const slides = this.rotationSlides();
    if (!slides?.length) return null;
    const idx = which === 0 ? this.rotIdx0() : this.rotIdx1();
    return slides[idx] ?? null;
  }

  private fetch() {
    const hwid = this.hardwareId();
    if (!hwid) return;
    this.api.getPlayerScreen(hwid, this.slot()).subscribe({
      next: (res) => {
        this.loadError.set(null);
        this.applyResponse(res);
      },
      error: (err) => {
        this.screen.set(null);
        this.rotationSlides.set(null);
        this.loadError.set(err?.error?.message ?? err?.message ?? '화면을 불러오지 못했습니다');
      },
    });
  }

  private applyResponse(res: PlayerScreenResponse) {
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

  private clearRotationTimers() {
    if (this.rotWaitTimer) clearTimeout(this.rotWaitTimer);
    if (this.rotFadeTimer) clearTimeout(this.rotFadeTimer);
    this.rotWaitTimer = null;
    this.rotFadeTimer = null;
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
}
