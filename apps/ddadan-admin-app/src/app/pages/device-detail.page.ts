import { Component, ElementRef, inject, OnInit, signal, viewChild } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { ActivatedRoute } from '@angular/router';
import { ApiService, DeviceView, MonitorView, ScreenView } from '../api.service';

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
              <div class="meta">
                Slot {{ m.slot }} · {{ m.resolutionW }}×{{ m.resolutionH }}
                <div class="screen-pick">
                  <select [ngModel]="m.currentScreenId" (ngModelChange)="assign(m, $event)">
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
        padding: 6px;
        min-width: 80px;
        min-height: 50px;
      }
      .meta {
        font-size: 11px;
        line-height: 1.3;
      }
      .screen-pick {
        margin-top: 4px;
      }
      .screen-pick select {
        background: #2c3753;
        color: #fff;
        border-color: #3d4b6e;
        width: 100%;
        font-size: 11px;
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

  // Pixels per displayed pixel (1 displayed px = `scale` real px on monitor)
  readonly scale = 8;
  private dragging: { id: number; offsetX: number; offsetY: number } | null = null;

  ngOnInit() {
    const id = Number(this.route.snapshot.paramMap.get('id'));
    this.api.getDevice(id).subscribe((d) => this.device.set(d));
    this.api.listScreens().subscribe((s) => this.screens.set(s));
  }

  startDrag(ev: MouseEvent, m: MonitorView) {
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
