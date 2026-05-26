import { Component, ElementRef, computed, inject, OnInit, signal, viewChild } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { ActivatedRoute } from '@angular/router';
import { ApiService, AssetView, DeviceView, MonitorView, ScreenLayoutItem, ScreenView } from '../api.service';

@Component({
  standalone: true,
  imports: [FormsModule],
  template: `
    <h1>디바이스 상세</h1>
    @if (device(); as d) {
      <div class="panel">
        <div class="row">
          <div>
            <strong>{{ d.name ?? '이름 없음' }}</strong>
            <span class="muted"> · {{ d.hardwareId }}</span>
          </div>
          <div class="muted">상태: {{ d.status }} · 마지막 접속 {{ d.lastSeenAt ?? '-' }}</div>
        </div>
        <p class="muted">캔버스에서 모니터를 드래그해 위치를 지정하세요. 위치는 해상도(px) 단위입니다.</p>
        <div #canvas class="canvas" (mouseup)="endDrag()" (mousemove)="onDrag($event)">
          @for (m of d.monitors; track m.id) {
            <div
              class="monitor"
              [style.left.px]="m.positionX / scale"
              [style.top.px]="m.positionY / scale"
              [style.width.px]="m.resolutionW / scale"
              [style.height.px]="m.resolutionH / scale"
              (mousedown)="startDrag($event, m)"
            >
              @if (screenFor(m); as scr) {
                <div
                  class="preview"
                  [style.background]="scr.layout.background ?? '#0c0f1a'"
                >
                  @for (item of scr.layout.items; track item.id) {
                    <div
                      class="prev-item"
                      [style.left.%]="(item.x / scr.width) * 100"
                      [style.top.%]="(item.y / scr.height) * 100"
                      [style.width.%]="(item.width / scr.width) * 100"
                      [style.height.%]="(item.height / scr.height) * 100"
                      [style.z-index]="item.zIndex ?? 1"
                      [style.background]="item.background ?? null"
                      [style.color]="item.color ?? null"
                    >
                      @switch (item.kind) {
                        @case ('image') {
                          @if (urlFor(item); as u) {
                            <img [src]="u" alt="" />
                          }
                        }
                        @case ('video') {
                          @if (urlFor(item); as u) {
                            <video [src]="u" autoplay muted loop playsinline></video>
                          }
                        }
                        @default {
                          <span class="prev-text" [style.font-size.px]="previewFontSize(item, scr)">{{ item.text }}</span>
                        }
                      }
                    </div>
                  }
                </div>
              } @else {
                <div class="preview empty">화면 없음</div>
              }
              <div class="overlay">
                <div class="overlay-top">Slot {{ m.slot }} · {{ m.resolutionW }}×{{ m.resolutionH }}</div>
                <div class="overlay-bottom screen-pick">
                  <select
                    [ngModel]="m.currentScreenId"
                    (ngModelChange)="assign(m, $event)"
                    (mousedown)="$event.stopPropagation()"
                  >
                    <option [ngValue]="null">— 기본 화면 —</option>
                    @for (s of screens(); track s.id) {
                      <option [ngValue]="s.id">{{ s.name }}</option>
                    }
                  </select>
                </div>
              </div>
            </div>
          }
        </div>
      </div>
    }
  `,
  styles: [
    `
      .row {
        display: flex;
        justify-content: space-between;
        margin-bottom: 12px;
      }
      .canvas {
        position: relative;
        width: 100%;
        height: 460px;
        background: #f0f3fa;
        border: 1px dashed var(--border);
        border-radius: 8px;
        overflow: hidden;
        margin-top: 10px;
      }
      .monitor {
        position: absolute;
        background: #1c2436;
        color: #fff;
        border-radius: 4px;
        cursor: move;
        user-select: none;
        box-shadow: 0 6px 16px rgba(0, 0, 0, 0.18);
        min-width: 120px;
        min-height: 80px;
        overflow: hidden;
      }
      .preview {
        position: absolute;
        inset: 0;
        overflow: hidden;
      }
      .preview.empty {
        display: flex;
        align-items: center;
        justify-content: center;
        font-size: 11px;
        color: rgba(255, 255, 255, 0.55);
        background: #1c2436;
      }
      .prev-item {
        position: absolute;
        overflow: hidden;
      }
      .prev-item img,
      .prev-item video {
        width: 100%;
        height: 100%;
        object-fit: cover;
        display: block;
        pointer-events: none;
      }
      .prev-text {
        display: flex;
        width: 100%;
        height: 100%;
        align-items: center;
        justify-content: center;
        text-align: center;
        line-height: 1.1;
        padding: 2px;
        box-sizing: border-box;
        overflow: hidden;
      }
      .overlay {
        position: absolute;
        inset: 0;
        display: flex;
        flex-direction: column;
        justify-content: space-between;
        pointer-events: none;
      }
      .overlay-top {
        font-size: 11px;
        line-height: 1.2;
        padding: 4px 6px;
        background: linear-gradient(to bottom, rgba(0, 0, 0, 0.55), rgba(0, 0, 0, 0));
        color: #fff;
      }
      .overlay-bottom {
        padding: 4px;
        background: linear-gradient(to top, rgba(0, 0, 0, 0.55), rgba(0, 0, 0, 0));
        pointer-events: auto;
      }
      .screen-pick select {
        background: rgba(20, 26, 40, 0.85);
        color: #fff;
        border: 1px solid rgba(255, 255, 255, 0.2);
        border-radius: 3px;
        width: 100%;
        font-size: 11px;
        padding: 2px 4px;
      }
    `,
  ],
})
export class DeviceDetailPage implements OnInit {
  private readonly api = inject(ApiService);
  private readonly route = inject(ActivatedRoute);
  readonly canvas = viewChild<ElementRef<HTMLDivElement>>('canvas');
  readonly device = signal<DeviceView | null>(null);
  readonly screens = signal<ScreenView[]>([]);
  readonly assets = signal<AssetView[]>([]);
  private readonly assetMap = computed(() => {
    const map = new Map<number, AssetView>();
    for (const a of this.assets()) map.set(a.id, a);
    return map;
  });

  // Pixels per displayed pixel (1 displayed px = `scale` real px on monitor)
  readonly scale = 8;
  private dragging: { id: number; offsetX: number; offsetY: number } | null = null;

  ngOnInit() {
    const id = Number(this.route.snapshot.paramMap.get('id'));
    this.api.getDevice(id).subscribe((d) => this.device.set(d));
    this.api.listScreens().subscribe((s) => this.screens.set(s));
    this.api.listAssets().subscribe((a) => this.assets.set(a));
  }

  screenFor(m: MonitorView): ScreenView | null {
    if (m.currentScreenId == null) return null;
    return this.screens().find((s) => s.id === m.currentScreenId) ?? null;
  }

  urlFor(item: ScreenLayoutItem): string | null {
    if (item.assetId == null) return null;
    const asset = this.assetMap().get(item.assetId);
    if (!asset?.url) return null;
    return this.api.absoluteAssetUrl(asset.url);
  }

  previewFontSize(item: ScreenLayoutItem, scr: ScreenView): number {
    const base = item.fontSize ?? 36;
    const ratio = this.scale > 0 ? 1 / this.scale : 1;
    return Math.max(8, Math.round(base * ratio));
  }

  startDrag(ev: MouseEvent, m: MonitorView) {
    const src = ev.target as HTMLElement | null;
    if (src?.closest('select, option, input, textarea, button, .screen-pick')) {
      return;
    }
    ev.preventDefault();
    const target = ev.currentTarget as HTMLDivElement;
    const rect = target.getBoundingClientRect();
    this.dragging = {
      id: m.id,
      offsetX: ev.clientX - rect.left,
      offsetY: ev.clientY - rect.top,
    };
  }

  onDrag(ev: MouseEvent) {
    if (!this.dragging) return;
    const canvas = this.canvas();
    if (!canvas) return;
    const rect = canvas.nativeElement.getBoundingClientRect();
    const x = ev.clientX - rect.left - this.dragging.offsetX;
    const y = ev.clientY - rect.top - this.dragging.offsetY;
    const dev = this.device();
    if (!dev) return;
    const updated = dev.monitors.map((m) =>
      m.id === this.dragging!.id
        ? { ...m, positionX: Math.max(0, x * this.scale), positionY: Math.max(0, y * this.scale) }
        : m,
    );
    this.device.set({ ...dev, monitors: updated });
  }

  endDrag() {
    if (!this.dragging) return;
    const id = this.dragging.id;
    this.dragging = null;
    const m = this.device()?.monitors.find((x) => x.id === id);
    if (!m) return;
    this.api.updateMonitorPosition(m.id, m.positionX, m.positionY).subscribe();
  }

  assign(monitor: MonitorView, raw: number | string | null) {
    const screenId =
      raw === null || raw === undefined || raw === ''
        ? null
        : typeof raw === 'number'
          ? raw
          : Number(raw);
    const sid = screenId !== null && !Number.isFinite(screenId) ? null : screenId;
    this.api.assignScreen(monitor.id, sid).subscribe({
      next: () => {
        const dev = this.device();
        if (!dev) return;
        this.device.set({
          ...dev,
          monitors: dev.monitors.map((m) =>
            m.id === monitor.id ? { ...m, currentScreenId: sid } : m,
          ),
        });
      },
      error: (err) => console.warn('assign screen failed', err),
    });
  }
}
