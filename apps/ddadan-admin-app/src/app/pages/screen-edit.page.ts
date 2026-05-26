import { Component, ElementRef, inject, OnInit, signal, viewChild } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { ActivatedRoute } from '@angular/router';
import { ApiService, AssetView, ScreenLayoutItem, ScreenView } from '../api.service';

function newId(): string {
  const c: Crypto | undefined = typeof crypto !== 'undefined' ? crypto : undefined;
  if (c && typeof c.randomUUID === 'function') return c.randomUUID();
  return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, (ch) => {
    const r = (Math.random() * 16) | 0;
    const v = ch === 'x' ? r : (r & 0x3) | 0x8;
    return v.toString(16);
  });
}

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
          <button (click)="save()" [disabled]="saveBusy()">
            {{ saveBusy() ? '저장 중…' : '저장' }}
          </button>
          @if (saveMessage(); as msg) {
            <span class="save-toast" [class.error]="saveError()">{{ msg }}</span>
          }
          @if (selected()) {
            <button class="secondary" (click)="saveAsComponent()">컴포넌트로 저장</button>
            <button class="secondary danger" (click)="removeSelected()">선택 삭제</button>
          }
        </div>
        <div class="editor">
          <div class="library">
            <h3>에셋</h3>
            <div class="library-add">
              <input
                #assetFile
                type="file"
                class="file-input"
                accept="image/*,video/*"
                (change)="onAssetFile($event)"
              />
              <button type="button" class="secondary" (click)="assetFile.click()">파일로 에셋 추가</button>
              <input [(ngModel)]="newTextName" placeholder="텍스트 이름" />
              <textarea [(ngModel)]="newTextBody" placeholder="텍스트 내용" rows="2"></textarea>
              <button
                type="button"
                class="secondary"
                (click)="addTextAsset()"
                [disabled]="!newTextName.trim() || !newTextBody.trim() || textBusy()"
              >
                텍스트 에셋 추가
              </button>
            </div>
            <div class="library-list">
              @for (a of assets(); track a.id) {
                <div class="library-item" (click)="addFromAsset(a)">
                  @switch (a.type) {
                    @case ('image') { <img [src]="api.absoluteAssetUrl(a.url)" alt="" /> }
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
      .library-add {
        display: flex;
        flex-direction: column;
        gap: 6px;
        margin-bottom: 10px;
        padding-bottom: 10px;
        border-bottom: 1px solid var(--border);
      }
      .library-add input,
      .library-add textarea {
        font-size: 12px;
      }
      .file-input {
        display: none;
      }
      .library-list {
        display: flex;
        flex-direction: column;
        gap: 6px;
        max-height: 180px;
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
      .save-toast {
        display: inline-flex;
        align-items: center;
        padding: 4px 10px;
        margin-left: 6px;
        background: #1f8c4d;
        color: #fff;
        border-radius: 12px;
        font-size: 12px;
        line-height: 1.4;
        animation: fadeIn 0.15s ease-out;
      }
      .save-toast.error {
        background: #c0392b;
      }
      @keyframes fadeIn {
        from {
          opacity: 0;
          transform: translateY(-2px);
        }
        to {
          opacity: 1;
          transform: translateY(0);
        }
      }
    `,
  ],
})
export class ScreenEditPage implements OnInit {
  readonly api = inject(ApiService);
  private readonly route = inject(ActivatedRoute);
  readonly stage = viewChild<ElementRef<HTMLDivElement>>('stage');
  readonly screen = signal<ScreenView | null>(null);
  readonly assets = signal<AssetView[]>([]);
  readonly components = signal<Array<{ id: number; name: string; kind: string; payload: ScreenLayoutItem | ScreenLayoutItem[] }>>([]);
  readonly selected = signal<ScreenLayoutItem | null>(null);
  newTextName = '';
  newTextBody = '';
  readonly textBusy = signal(false);

  private drag: DragState | null = null;

  ngOnInit() {
    const id = Number(this.route.snapshot.paramMap.get('id'));
    this.api.getScreen(id).subscribe((s) => this.screen.set(s));
    this.refreshAssets();
    this.api.listComponents().subscribe((c) => this.components.set(c));
  }

  private refreshAssets() {
    this.api.listAssets().subscribe((a) => this.assets.set(a));
  }

  onAssetFile(ev: Event) {
    const s = this.screen();
    if (!s) return;
    const input = ev.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    this.api.uploadAsset(file, s.storeId ?? undefined).subscribe({
      next: (asset) => {
        input.value = '';
        this.refreshAssets();
        this.addFromAsset(asset);
      },
      error: () => (input.value = ''),
    });
  }

  addTextAsset() {
    const s = this.screen();
    if (!s) return;
    const name = this.newTextName.trim();
    const body = this.newTextBody.trim();
    if (!name || !body) return;
    this.textBusy.set(true);
    this.api.createTextAsset(name, body).subscribe({
      next: (asset) => {
        this.newTextName = '';
        this.newTextBody = '';
        this.textBusy.set(false);
        this.refreshAssets();
        this.addFromAsset(asset);
      },
      error: () => this.textBusy.set(false),
    });
  }

  aspectRatio(): string {
    const s = this.screen();
    return s ? `${s.width} / ${s.height}` : '16 / 9';
  }

  urlOf(item: ScreenLayoutItem): string {
    if (!item.assetId) return '';
    const asset = this.assets().find((a) => a.id === item.assetId);
    return this.api.absoluteAssetUrl(asset?.url ?? null);
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
      id: newId(),
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
    const cloned = items.map((i) => ({ ...i, id: newId() }));
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

  private dragMoveListener: ((ev: MouseEvent) => void) | null = null;
  private dragUpListener: (() => void) | null = null;

  startDrag(ev: MouseEvent, item: ScreenLayoutItem, mode: 'move' | 'resize') {
    ev.stopPropagation();
    ev.preventDefault();
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
    this.detachDragListeners();
    const move = (e: MouseEvent) => this.onDrag(e);
    const up = () => {
      this.endDrag();
      this.detachDragListeners();
    };
    this.dragMoveListener = move;
    this.dragUpListener = up;
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', up);
  }

  private detachDragListeners() {
    if (this.dragMoveListener) {
      window.removeEventListener('mousemove', this.dragMoveListener);
      this.dragMoveListener = null;
    }
    if (this.dragUpListener) {
      window.removeEventListener('mouseup', this.dragUpListener);
      this.dragUpListener = null;
    }
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
      const startW = this.drag!.startItemW;
      const startH = this.drag!.startItemH;
      let newW = Math.max(40, startW + dx);
      let newH = Math.max(40, startH + dy);
      const preserveAspect = !ev.shiftKey && startW > 0 && startH > 0;
      if (preserveAspect) {
        const aspect = startW / startH;
        if (Math.abs(dx) * startH >= Math.abs(dy) * startW) {
          newH = Math.max(40, newW / aspect);
        } else {
          newW = Math.max(40, newH * aspect);
        }
      }
      return {
        ...i,
        width: Math.round(newW),
        height: Math.round(newH),
      };
    });
    this.screen.set({ ...s, layout: { ...s.layout, items } });
    const updated = items.find((i) => i.id === this.drag!.itemId);
    if (updated) this.selected.set(updated);
  }

  endDrag() {
    this.drag = null;
  }

  readonly saveBusy = signal(false);
  readonly saveMessage = signal<string | null>(null);
  readonly saveError = signal(false);
  private saveMsgTimer: ReturnType<typeof setTimeout> | null = null;

  save() {
    const s = this.screen();
    if (!s) return;
    this.saveBusy.set(true);
    this.saveMessage.set(null);
    this.saveError.set(false);
    if (this.saveMsgTimer) clearTimeout(this.saveMsgTimer);
    this.api
      .updateScreen(s.id, { name: s.name, width: s.width, height: s.height, layout: s.layout })
      .subscribe({
        next: () => {
          this.saveBusy.set(false);
          this.saveError.set(false);
          this.saveMessage.set('저장 완료');
          this.saveMsgTimer = setTimeout(() => this.saveMessage.set(null), 2000);
        },
        error: (err) => {
          this.saveBusy.set(false);
          this.saveError.set(true);
          this.saveMessage.set(`저장 실패: ${err?.error?.message ?? err?.message ?? '오류'}`);
          this.saveMsgTimer = setTimeout(() => this.saveMessage.set(null), 4000);
        },
      });
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
