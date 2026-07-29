import { Component, ElementRef, computed, inject, OnInit, signal, viewChild } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { ActivatedRoute } from '@angular/router';
import { ApiService, AssetView, CommandView, DeviceView, MonitorView, ScreenLayoutItem, ScreenshotView, ScreenView } from '../api.service';
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

        <div class="telemetry">
          <span class="tbadge" [class.on]="d.status === 'online'">{{ d.status }}</span>
          @if (d.appVersion) { <span class="tchip">v{{ d.appVersion }}</span> }
          @if (d.cpuPercent != null) { <span class="tchip">CPU {{ d.cpuPercent }}%</span> }
          @if (d.ramUsedMb != null) { <span class="tchip">RAM {{ d.ramUsedMb }}/{{ d.ramTotalMb }}MB</span> }
          @if (d.diskUsedPercent != null) {
            <span class="tchip" [class.warn]="d.diskUsedPercent >= 80">디스크 {{ d.diskUsedPercent }}%</span>
          }
        </div>

        <div class="cmd-row">
          <button (click)="send('reboot')">재부팅</button>
          <button (click)="send('screenOff')">화면끄기</button>
          <button (click)="send('screenOn')">화면켜기</button>
          <button (click)="send('updateApp')">앱 업데이트</button>
          <input class="shell" [(ngModel)]="shellInput" placeholder="root shell 명령" />
          <button (click)="sendShell()">실행</button>
        </div>

        @if (commands().length) {
          <table class="cmd-history">
            <tr><th>명령</th><th>상태</th><th>결과</th><th>예약시각</th></tr>
            @for (c of commands(); track c.id) {
              <tr>
                <td>{{ c.type }}{{ c.payload ? ' (' + c.payload + ')' : '' }}</td>
                <td [class.done]="c.status === 'done'" [class.failed]="c.status === 'failed'">{{ c.status }}</td>
                <td class="result">{{ c.result }}</td>
                <td class="muted">{{ c.createdAt }}</td>
              </tr>
            }
          </table>
        }

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
        <div class="shots-head">
          <h2 class="shots-title">최근 스크린샷</h2>
          <button (click)="send('screenshot')">지금 캡처 요청</button>
        </div>
        @if (screenshots().length === 0) {
          <p class="muted">아직 업로드된 스크린샷이 없습니다. "지금 캡처 요청"을 누르면 박스가 현재 화면을 캡처해 올립니다.</p>
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
      .telemetry { display: flex; gap: 8px; flex-wrap: wrap; align-items: center; margin: 4px 0 10px; }
      .tbadge { font-size: 12px; font-weight: 700; padding: 3px 10px; border-radius: 999px; background: #6b7280; color: #fff; }
      .tbadge.on { background: #22a06b; }
      .tchip { font-size: 12px; color: var(--muted, #8a93a6); border: 1px solid var(--border); border-radius: 6px; padding: 3px 8px; }
      /* 디스크 임계(80%) 초과 — 워치독은 85%에서 자동 정리하지만 눈으로도 보이게. */
      .tchip.warn { color: #b42318; border-color: #f3a29b; background: #fff4f2; font-weight: 700; }
      .cmd-row { display: flex; gap: 8px; flex-wrap: wrap; align-items: center; margin-bottom: 10px; }
      .cmd-row button { padding: 6px 12px; border-radius: 6px; font-size: 13px; cursor: pointer; }
      .cmd-row .shell { flex: 1 1 200px; min-width: 160px; padding: 6px 8px; border: 1px solid var(--border); border-radius: 6px; }
      .cmd-history { width: 100%; border-collapse: collapse; font-size: 12px; margin-bottom: 12px; }
      .cmd-history th, .cmd-history td { text-align: left; padding: 4px 8px; border-bottom: 1px solid var(--border); vertical-align: top; }
      .cmd-history td.done { color: #22a06b; }
      .cmd-history td.failed { color: #e0552f; }
      .cmd-history td.result { font-family: monospace; white-space: pre-wrap; word-break: break-all; max-width: 320px; }
      .shots-title { margin: 0 0 10px; font-size: 15px; }
      .shots-head { display: flex; align-items: center; justify-content: space-between; gap: 10px; margin-bottom: 10px; }
      .shots-head .shots-title { margin: 0; }
      .shots-head button { padding: 6px 12px; border-radius: 6px; font-size: 13px; cursor: pointer; }
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
  readonly commands = signal<CommandView[]>([]);
  shellInput = '';
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
    this.refreshCommands(id);
  }

  previewUrl(d: DeviceView): string {
    return `${window.location.origin}/preview/${encodeURIComponent(d.hardwareId)}`;
  }

  shotUrl(url: string): string {
    return this.api.absoluteAssetUrl(url);
  }

  private deviceId(): number {
    return this.device()?.id ?? Number(this.route.snapshot.paramMap.get('id'));
  }

  private refreshCommands(id = this.deviceId()) {
    this.api.listCommands(id).subscribe((c) => this.commands.set(c));
  }

  send(type: CommandView['type'], payload?: string) {
    this.api.sendCommand(this.deviceId(), type, payload).subscribe(() => {
      setTimeout(() => this.refreshCommands(), 300);
    });
  }

  sendShell() {
    const cmd = this.shellInput.trim();
    if (!cmd) return;
    this.send('shell', cmd);
    this.shellInput = '';
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
