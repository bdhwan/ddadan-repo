import { Component, ElementRef, inject, OnInit, signal, viewChild } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { ActivatedRoute } from '@angular/router';
import { ApiService, AssetView, ScreenLayoutItem, ScreenView } from '../api.service';

interface DragState {
  itemId: string;
  mode: 'move' | 'resize';
  startX: number;
  startY: number;
  startItemX: number;
  startItemY: number;
  startItemW: number;
  startItemH: number;
}

@Component({
  standalone: true,
  imports: [FormsModule],
  template: `
    <h1>화면 편집</h1>
    @if (screen(); as s) {
      <div class="panel">
        <div class="toolbar">
          <input [(ngModel)]="s.name" />
          <span class="muted">{{ s.width }} × {{ s.height }}</span>
          <button (click)="save()">저장</button>
          @if (selected()) {
            <button class="secondary" (click)="saveAsComponent()">컴포넌트로 저장</button>
            <button class="secondary danger" (click)="removeSelected()">선택 삭제</button>
          }
        </div>
        <div class="editor">
          <div class="library">
            <h3>에셋</h3>
            <div class="library-list">
              @for (a of assets(); track a.id) {
                <div class="library-item" (click)="addFromAsset(a)">
                  @switch (a.type) {
                    @case ('image') { <img [src]="a.url ?? ''" alt="" /> }
                    @case ('video') { <span>🎬</span> }
                    @default { <span>📝</span> }
                  }
                  <div class="name">{{ a.originalName }}</div>
                </div>
              }
            </div>
            <h3>저장된 컴포넌트</h3>
            <div class="library-list">
              @for (c of components(); track c.id) {
                <div class="library-item" (click)="addFromComponent(c)">
                  <span>🧩</span>
                  <div class="name">{{ c.name }}</div>
                </div>
              }
            </div>
          </div>
          <div
            #stage
            class="stage"
            [style.aspect-ratio]="aspectRatio()"
            (mousemove)="onDrag($event)"
            (mouseup)="endDrag()"
            (click)="clearSelection($event)"
          >
            @for (item of s.layout.items; track item.id) {
              <div
                class="item"
                [class.selected]="selected()?.id === item.id"
                [style.left.%]="(item.x / s.width) * 100"
                [style.top.%]="(item.y / s.height) * 100"
                [style.width.%]="(item.width / s.width) * 100"
                [style.height.%]="(item.height / s.height) * 100"
                [style.background]="item.background ?? null"
                [style.color]="item.color ?? null"
                [style.font-size.px]="item.fontSize ?? null"
                [style.z-index]="item.zIndex ?? 1"
                (click)="select($event, item)"
                (mousedown)="startDrag($event, item, 'move')"
              >
                @switch (item.kind) {
                  @case ('image') { <img [src]="urlOf(item)" alt="" /> }
                  @case ('video') { <video [src]="urlOf(item)" muted autoplay loop></video> }
                  @case ('text') { <div class="text">{{ item.text }}</div> }
                }
                <div class="handle" (mousedown)="startDrag($event, item, 'resize')"></div>
              </div>
            }
          </div>
          <div class="inspector">
            <h3>속성</h3>
            @if (selected(); as it) {
              <label>X</label>
              <input type="number" [ngModel]="it.x" (ngModelChange)="patch(it, 'x', $event)" />
              <label>Y</label>
              <input type="number" [ngModel]="it.y" (ngModelChange)="patch(it, 'y', $event)" />
              <label>너비</label>
              <input type="number" [ngModel]="it.width" (ngModelChange)="patch(it, 'width', $event)" />
              <label>높이</label>
              <input type="number" [ngModel]="it.height" (ngModelChange)="patch(it, 'height', $event)" />
              @if (it.kind === 'text') {
                <label>텍스트</label>
                <input [ngModel]="it.text" (ngModelChange)="patch(it, 'text', $event)" />
                <label>글자 크기</label>
                <input type="number" [ngModel]="it.fontSize" (ngModelChange)="patch(it, 'fontSize', $event)" />
                <label>색</label>
                <input [ngModel]="it.color" (ngModelChange)="patch(it, 'color', $event)" />
              }
            } @else {
              <p class="muted">선택된 항목이 없습니다.</p>
            }
          </div>
        </div>
      </div>
    }
  `,
  styles: [
    `
      .editor {
        display: grid;
        grid-template-columns: 220px 1fr 240px;
        gap: 12px;
        margin-top: 12px;
      }
      .library,
      .inspector {
        background: #fafbfd;
        padding: 10px;
        border-radius: 8px;
        border: 1px solid var(--border);
      }
      .library h3,
      .inspector h3 {
        font-size: 13px;
        margin: 6px 0 6px;
      }
      .library-list {
        display: flex;
        flex-direction: column;
        gap: 6px;
        max-height: 220px;
        overflow: auto;
      }
      .library-item {
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 4px 6px;
        border: 1px solid var(--border);
        border-radius: 6px;
        background: #fff;
        cursor: pointer;
      }
      .library-item img {
        width: 36px;
        height: 24px;
        object-fit: cover;
        border-radius: 3px;
      }
      .library-item .name {
        font-size: 12px;
        line-height: 1.2;
        word-break: break-all;
      }
      .stage {
        position: relative;
        background: #1c2436;
        border-radius: 8px;
        overflow: hidden;
      }
      .item {
        position: absolute;
        background: rgba(255, 255, 255, 0.05);
        border: 1px solid transparent;
        cursor: move;
        overflow: hidden;
      }
      .item.selected {
        border-color: var(--accent);
        box-shadow: 0 0 0 2px rgba(42, 108, 255, 0.4);
      }
      .item img,
      .item video {
        width: 100%;
        height: 100%;
        object-fit: cover;
        pointer-events: none;
      }
      .item .text {
        padding: 6px;
        white-space: pre-wrap;
      }
      .handle {
        position: absolute;
        right: -4px;
        bottom: -4px;
        width: 12px;
        height: 12px;
        background: var(--accent);
        border-radius: 50%;
        cursor: nwse-resize;
      }
    `,
  ],
})
export class ScreenEditPage implements OnInit {
  private readonly api = inject(ApiService);
  private readonly route = inject(ActivatedRoute);
  readonly stage = viewChild<ElementRef<HTMLDivElement>>('stage');
  readonly screen = signal<ScreenView | null>(null);
  readonly assets = signal<AssetView[]>([]);
  readonly components = signal<Array<{ id: number; name: string; kind: string; payload: ScreenLayoutItem | ScreenLayoutItem[] }>>([]);
  readonly selected = signal<ScreenLayoutItem | null>(null);

  private drag: DragState | null = null;

  ngOnInit() {
    const id = Number(this.route.snapshot.paramMap.get('id'));
    this.api.getScreen(id).subscribe((s) => this.screen.set(s));
    this.api.listAssets().subscribe((a) => this.assets.set(a));
    this.api.listComponents().subscribe((c) => this.components.set(c));
  }

  aspectRatio(): string {
    const s = this.screen();
    return s ? `${s.width} / ${s.height}` : '16 / 9';
  }

  urlOf(item: ScreenLayoutItem): string {
    if (!item.assetId) return '';
    const asset = this.assets().find((a) => a.id === item.assetId);
    return asset?.url ?? '';
  }

  select(ev: MouseEvent, item: ScreenLayoutItem) {
    ev.stopPropagation();
    this.selected.set(item);
  }

  clearSelection(ev: MouseEvent) {
    if (ev.target === ev.currentTarget) {
      this.selected.set(null);
    }
  }

  patch<K extends keyof ScreenLayoutItem>(item: ScreenLayoutItem, key: K, value: ScreenLayoutItem[K]) {
    const s = this.screen();
    if (!s) return;
    const items = s.layout.items.map((i) => (i.id === item.id ? { ...i, [key]: value } : i));
    this.screen.set({ ...s, layout: { ...s.layout, items } });
    const updated = items.find((i) => i.id === item.id);
    if (updated) this.selected.set(updated);
  }

  addFromAsset(a: AssetView) {
    const s = this.screen();
    if (!s) return;
    const item: ScreenLayoutItem = {
      id: crypto.randomUUID(),
      kind: a.type,
      assetId: a.id,
      x: 100,
      y: 100,
      width: a.type === 'text' ? 600 : 400,
      height: a.type === 'text' ? 200 : 300,
      ...(a.type === 'text'
        ? { text: a.textContent ?? '', fontSize: 36, color: '#ffffff' }
        : {}),
    };
    this.screen.set({ ...s, layout: { ...s.layout, items: [...s.layout.items, item] } });
    this.selected.set(item);
  }

  addFromComponent(c: { kind: string; payload: ScreenLayoutItem | ScreenLayoutItem[] }) {
    const s = this.screen();
    if (!s) return;
    const items = Array.isArray(c.payload) ? c.payload : [c.payload];
    const cloned = items.map((i) => ({ ...i, id: crypto.randomUUID() }));
    this.screen.set({
      ...s,
      layout: { ...s.layout, items: [...s.layout.items, ...cloned] },
    });
  }

  removeSelected() {
    const sel = this.selected();
    const s = this.screen();
    if (!sel || !s) return;
    this.screen.set({
      ...s,
      layout: { ...s.layout, items: s.layout.items.filter((i) => i.id !== sel.id) },
    });
    this.selected.set(null);
  }

  startDrag(ev: MouseEvent, item: ScreenLayoutItem, mode: 'move' | 'resize') {
    ev.stopPropagation();
    this.selected.set(item);
    this.drag = {
      itemId: item.id,
      mode,
      startX: ev.clientX,
      startY: ev.clientY,
      startItemX: item.x,
      startItemY: item.y,
      startItemW: item.width,
      startItemH: item.height,
    };
  }

  onDrag(ev: MouseEvent) {
    if (!this.drag) return;
    const s = this.screen();
    const stage = this.stage();
    if (!s || !stage) return;
    const rect = stage.nativeElement.getBoundingClientRect();
    const scaleX = s.width / rect.width;
    const scaleY = s.height / rect.height;
    const dx = (ev.clientX - this.drag.startX) * scaleX;
    const dy = (ev.clientY - this.drag.startY) * scaleY;
    const items = s.layout.items.map((i) => {
      if (i.id !== this.drag!.itemId) return i;
      if (this.drag!.mode === 'move') {
        return { ...i, x: Math.round(this.drag!.startItemX + dx), y: Math.round(this.drag!.startItemY + dy) };
      }
      return {
        ...i,
        width: Math.max(40, Math.round(this.drag!.startItemW + dx)),
        height: Math.max(40, Math.round(this.drag!.startItemH + dy)),
      };
    });
    this.screen.set({ ...s, layout: { ...s.layout, items } });
    const updated = items.find((i) => i.id === this.drag!.itemId);
    if (updated) this.selected.set(updated);
  }

  endDrag() {
    this.drag = null;
  }

  save() {
    const s = this.screen();
    if (!s) return;
    this.api
      .updateScreen(s.id, { name: s.name, width: s.width, height: s.height, layout: s.layout })
      .subscribe();
  }

  saveAsComponent() {
    const sel = this.selected();
    if (!sel) return;
    const name = prompt('컴포넌트 이름', sel.kind);
    if (!name) return;
    this.api.saveComponent(name, sel).subscribe(() => {
      this.api.listComponents().subscribe((c) => this.components.set(c));
    });
  }
}
