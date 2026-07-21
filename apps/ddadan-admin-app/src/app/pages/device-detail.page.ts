import { Component, ElementRef, computed, inject, OnInit, signal, viewChild } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { ActivatedRoute } from '@angular/router';
import { ApiService, AssetView, DeviceView, MonitorView, ScreenLayoutItem, ScreenshotView, ScreenView } from '../api.service';
import {
  MonitorRotationPanelComponent,
  RotForm,
} from '../components/monitor-rotation-panel.component';
import { isMenuLine, textAlignStyle } from '../screen-utils';

@Component({
  standalone: true,
  imports: [FormsModule, MonitorRotationPanelComponent],
  template: `
    <h1>디바이스 상세</h1>
    @if (device(); as d) {
      <div class="panel">
        <div class="row">
          <div>
            <strong>{{ d.name ?? '이름 없음' }}</strong>
            <span class="muted"> · {{ d.hardwareId }}</span>
          </div>
          <div class="row-actions">
            <a class="secondary btn-link" [href]="previewUrl(d)" target="_blank" rel="noopener">화면 미리보기</a>
            <span class="muted">상태: {{ d.status }} · 마지막 접속 {{ d.lastSeenAt ?? '-' }}</span>
          </div>
        </div>
        <p class="muted">캔버스에서 모니터를 드래그해 위치를 지정하세요. 각 모니터 하단에서 화면·로테이션을 설정합니다.</p>
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
                <div class="preview" [style.background]="scr.layout.background ?? '#0c0f1a'">
                  @for (item of sortedItems(scr); track item.id) {
                    <div
                      class="prev-item"
                      [style.left.%]="(item.x / scr.width) * 100"
                      [style.top.%]="(item.y / scr.height) * 100"
                      [style.width.%]="(item.width / scr.width) * 100"
                      [style.height.%]="(item.height / scr.height) * 100"
                      [style.z-index]="item.zIndex ?? 1"
                      [style.background]="item.background ?? null"
                      [style.color]="item.color ?? null"
                      [style.font-weight]="item.fontWeight ?? null"
                      [style.text-align]="textAlignStyle(item)"
                      [style.opacity]="item.opacity ?? null"
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
                          @if (isMenuLine(item)) {
                            <span class="prev-text menu-line">
                              <span class="label">{{ item.text }}</span>
                              <span class="dots"></span>
                              <span class="price">{{ item.textSecondary }}</span>
                            </span>
                          } @else {
                            <span class="prev-text" [style.font-size.px]="previewFontSize(item, scr)">{{ item.text }}</span>
                          }
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
                <div class="overlay-bottom">
                  <app-monitor-rotation-panel
                    [monitor]="m"
                    [screens]="screens()"
                    [assets]="assets()"
                    [form]="form(m.id)"
                    (formChange)="patchForm(m.id, $event)"
                    (apply)="applyRotation(m.id)"
                  />
                </div>
              </div>
            </div>
          }
        </div>
      </div>

      <div class="panel">
        <h2 class="shots-title">최근 스크린샷</h2>
        @if (screenshots().length === 0) {
          <p class="muted">아직 업로드된 스크린샷이 없습니다. (플레이어가 30초마다 현재 화면을 업로드합니다)</p>
        } @else {
          <div class="shots">
            @for (s of screenshots(); track s.id) {
              <figure class="shot">
                <img [src]="shotUrl(s.url)" alt="screenshot" loading="lazy" />
                <figcaption>{{ s.createdAt }}</figcaption>
              </figure>
            }
          </div>
        }
      </div>
    }
  `,
  styles: [
    `
      .shots-title { margin: 0 0 10px; font-size: 15px; }
      .shots {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
        gap: 12px;
      }
      .shot {
        margin: 0;
        border: 1px solid var(--border);
        border-radius: 8px;
        overflow: hidden;
        background: #0c0f1a;
      }
      .shot img {
        width: 100%;
        aspect-ratio: 16 / 9;
        object-fit: cover;
        display: block;
      }
      .shot figcaption {
        font-size: 11px;
        color: var(--muted, #8a93a6);
        padding: 6px 8px;
        word-break: break-all;
      }
      .row {
        display: flex;
        justify-content: space-between;
        align-items: flex-start;
        gap: 12px;
        margin-bottom: 12px;
        flex-wrap: wrap;
      }
      .row-actions { display: flex; flex-direction: column; align-items: flex-end; gap: 4px; }
      .btn-link {
        display: inline-block;
        padding: 6px 12px;
        border-radius: 6px;
        text-decoration: none;
        font-size: 12px;
        font-weight: 600;
      }
      .canvas {
        position: relative;
        width: 100%;
        height: 520px;
        background: #f0f3fa;
        border: 1px dashed var(--border);
        border-radius: 8px;
        overflow: auto;
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
        min-width: 280px;
        min-height: 200px;
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
      .prev-text.menu-line {
        align-items: baseline;
        justify-content: flex-start;
        gap: 4px;
        padding: 2px 4px;
      }
      .prev-text.menu-line .label { flex: 0 1 auto; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
      .prev-text.menu-line .dots { flex: 1 1 auto; border-bottom: 1px dotted currentColor; opacity: 0.45; min-width: 8px; }
      .prev-text.menu-line .price { flex: 0 0 auto; white-space: nowrap; }
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
        padding: 6px;
        background: linear-gradient(to top, rgba(0, 0, 0, 0.82), rgba(0, 0, 0, 0.35));
        pointer-events: auto;
        max-height: 55%;
        overflow: auto;
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
  readonly screenshots = signal<ScreenshotView[]>([]);
  readonly rotForms = signal<Record<number, RotForm>>({});
  private readonly assetMap = computed(() => {
    const map = new Map<number, AssetView>();
    for (const a of this.assets()) map.set(a.id, a);
    return map;
  });

  readonly scale = 8;
  private dragging: { id: number; offsetX: number; offsetY: number } | null = null;

  isMenuLine = isMenuLine;
  textAlignStyle = textAlignStyle;

  ngOnInit() {
    const id = Number(this.route.snapshot.paramMap.get('id'));
    this.api.getDevice(id).subscribe((d) => {
      this.device.set(d);
      this.rotForms.update((prev) => {
        const next = { ...prev };
        for (const m of d.monitors) next[m.id] = this.initForm(m);
        return next;
      });
    });
    this.api.listScreens().subscribe((s) => this.screens.set(s));
    this.api.listAssets().subscribe((a) => this.assets.set(a));
    this.api.listDeviceScreenshots(id).subscribe((s) => this.screenshots.set(s));
  }

  previewUrl(d: DeviceView): string {
    return `${window.location.origin}/preview/${encodeURIComponent(d.hardwareId)}`;
  }

  shotUrl(url: string): string {
    return this.api.absoluteAssetUrl(url);
  }

  sortedItems(scr: ScreenView): ScreenLayoutItem[] {
    return [...scr.layout.items].sort((a, b) => (a.zIndex ?? 1) - (b.zIndex ?? 1));
  }

  form(monitorId: number): RotForm {
    return this.rotForms()[monitorId] ?? { screenIds: [], intervalSec: 10, fadeMs: 800 };
  }

  private initForm(m: MonitorView): RotForm {
    let screenIds: number[] = [];
    const rot = m.rotationScreenIds ?? [];
    if (rot.length >= 2) {
      screenIds = [...rot];
    } else if (m.currentScreenId != null) {
      screenIds = [m.currentScreenId];
    } else if (rot.length === 1) {
      screenIds = [...rot];
    }
    return {
      screenIds,
      intervalSec: Math.max(2, Math.round((m.rotationIntervalMs ?? 10_000) / 1000)),
      fadeMs: Math.max(200, m.rotationFadeMs ?? 800),
    };
  }

  patchForm(monitorId: number, form: RotForm) {
    this.rotForms.update((forms) => ({ ...forms, [monitorId]: form }));
  }

  applyRotation(monitorId: number) {
    const f = this.form(monitorId);
    this.api
      .setMonitorRotation(monitorId, {
        screenIds: f.screenIds,
        intervalMs: Math.max(2000, Math.round(f.intervalSec * 1000)),
        fadeMs: Math.max(200, f.fadeMs),
      })
      .subscribe(() => {
        const id = Number(this.route.snapshot.paramMap.get('id'));
        this.api.getDevice(id).subscribe((d) => {
          this.device.set(d);
          const m = d.monitors.find((x) => x.id === monitorId);
          if (m) this.rotForms.update((forms) => ({ ...forms, [monitorId]: this.initForm(m) }));
        });
      });
  }

  screenFor(m: MonitorView): ScreenView | null {
    const rot = m.rotationScreenIds ?? [];
    const id = rot.length >= 1 ? rot[0] : m.currentScreenId;
    if (id == null) return null;
    return this.screens().find((s) => s.id === id) ?? null;
  }

  urlFor(item: ScreenLayoutItem): string | null {
    if (item.assetId == null) return null;
    const asset = this.assetMap().get(item.assetId);
    if (!asset?.url) return null;
    return this.api.absoluteAssetUrl(asset.url);
  }

  previewFontSize(item: ScreenLayoutItem, scr: ScreenView): number {
    const stored = item.fontSize ?? 36;
    const base = item.fontUnit === 'vh' ? (stored * scr.height) / 100 : stored;
    const ratio = this.scale > 0 ? 1 / this.scale : 1;
    return Math.max(8, Math.round(base * ratio));
  }

  startDrag(ev: MouseEvent, m: MonitorView) {
    const src = ev.target as HTMLElement | null;
    if (src?.closest('select, option, input, textarea, button, label, .rot-panel, a')) return;
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
}
