import { HttpClient } from '@angular/common/http';
import { Component, computed, inject, OnDestroy, OnInit, signal } from '@angular/core';
import { environment } from '../environment';

interface ScreenItem {
  id: string;
  kind: 'image' | 'video' | 'text';
  url?: string;
  text?: string;
  fontSize?: number;
  color?: string;
  background?: string;
  x: number;
  y: number;
  width: number;
  height: number;
  zIndex?: number;
}

interface ScreenResponse {
  registered: boolean;
  deviceName: string | null;
  width: number;
  height: number;
  background?: string;
  items: ScreenItem[];
  isFallback?: boolean;
}

@Component({
  selector: 'app-root',
  templateUrl: './app.html',
  styleUrl: './app.scss',
})
export class App implements OnInit, OnDestroy {
  private readonly http = inject(HttpClient);
  protected readonly screen = signal<ScreenResponse | null>(null);
  protected readonly hardwareId = signal('');
  protected readonly slot = signal(0);
  protected readonly aspect = computed(() => {
    const s = this.screen();
    return s ? `${s.width} / ${s.height}` : '16 / 9';
  });

  private timer: ReturnType<typeof setInterval> | null = null;

  ngOnInit(): void {
    const url = new URL(window.location.href);
    const deviceId = url.searchParams.get('deviceId') ?? 'dev-local';
    const slot = Number(url.searchParams.get('slot') ?? '0');
    this.hardwareId.set(deviceId);
    this.slot.set(slot);

    this.fetch();
    this.timer = setInterval(() => this.fetch(), environment.pollIntervalMs);
  }

  ngOnDestroy(): void {
    if (this.timer) clearInterval(this.timer);
  }

  protected absoluteUrl(item: ScreenItem): string | null {
    if (!item.url) return null;
    if (item.url.startsWith('http')) return item.url;
    return `${environment.apiBase.replace(/\/api$/, '')}${item.url}`;
  }

  private fetch() {
    const hwid = this.hardwareId();
    if (!hwid) return;
    this.http
      .get<ScreenResponse>(`${environment.apiBase}/player/${encodeURIComponent(hwid)}/screen?slot=${this.slot()}`)
      .subscribe({
        next: (res) => this.screen.set(res),
        error: (err) => console.warn('player fetch failed', err.message),
      });
  }
}
