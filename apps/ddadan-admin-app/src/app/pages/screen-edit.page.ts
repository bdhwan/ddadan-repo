import {
  Component,
  ElementRef,
  HostListener,
  inject,
  OnDestroy,
  OnInit,
  signal,
  viewChild,
} from '@angular/core';
import { DecimalPipe } from '@angular/common';
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
  imports: [FormsModule, DecimalPipe],
  template: `
    <h1>화면 편집</h1>
    @if (screen(); as s) {
      <div class="panel">
        <div class="toolbar">
          <input [(ngModel)]="s.name" (ngModelChange)="markDirty()" />
          <span class="muted">{{ s.width }} × {{ s.height }}</span>
          <button (click)="save()" [disabled]="saveBusy()">
            {{ saveBusy() ? '저장 중…' : '저장' }}
          </button>
          <span class="save-state" [class.dirty]="dirty()">
            {{ dirty() ? '● 저장되지 않은 변경' : '✓ 저장됨' }}
          </span>
          @if (saveMessage(); as msg) {
            <span class="save-toast" [class.error]="saveError()">{{ msg }}</span>
          }
          <span class="spacer"></span>
          @if (selected()) {
            <button class="secondary" (click)="duplicateSelected()">복제</button>
            <button class="secondary" (click)="saveAsComponent()">컴포넌트로 저장</button>
            <button class="secondary danger" (click)="removeSelected()">선택 삭제</button>
          }
        </div>

        <div class="bg-bar">
          <span class="bg-label">화면 배경</span>
          <input type="color" [ngModel]="bgColor()" (ngModelChange)="setScreenBg($event)" title="배경 색" />
          <input
            class="bg-hex"
            [ngModel]="s.layout.background ?? ''"
            (ngModelChange)="setScreenBg($event)"
            placeholder="#0c0f1a 또는 비움"
          />
          <button class="secondary" (click)="setScreenBg('')">배경 지우기</button>
          <span class="div"></span>
          <button class="secondary" (click)="addDimLayer()">＋ 딤 레이어</button>
          <span class="hint muted">이미지를 배경으로: 선택 후 “화면 채우기” → “맨 뒤로”</span>
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
                multiple
                (change)="onAssetFiles($event)"
              />
              <button type="button" class="secondary" (click)="assetFile.click()">
                파일로 에셋 추가 (다중)
              </button>
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
                <button type="button" class="library-item" (click)="addFromAsset(a)">
                  @switch (a.type) {
                    @case ('image') { <img [src]="api.absoluteAssetUrl(a.url)" alt="" /> }
                    @case ('video') { <span>🎬</span> }
                    @default { <span>📝</span> }
                  }
                  <div class="name">{{ a.originalName }}</div>
                </button>
              }
            </div>
            <h3>저장된 컴포넌트</h3>
            <div class="library-list">
              @for (c of components(); track c.id) {
                <button type="button" class="library-item" (click)="addFromComponent(c)">
                  <span>🧩</span>
                  <div class="name">{{ c.name }}</div>
                </button>
              }
            </div>
          </div>

          <div
            #stage
            class="stage"
            [style.aspect-ratio]="aspectRatio()"
            [style.background]="s.layout.background || '#1c2436'"
            (click)="clearSelection($event)"
          >
            @for (item of sortedItems(); track item.id) {
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
                [style.font-weight]="item.fontWeight ?? null"
                [style.text-align]="item.textAlign ?? null"
                [style.line-height]="item.lineHeight ?? null"
                [style.opacity]="item.opacity ?? null"
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
              <div class="quick">
                <button class="secondary" (click)="fitToScreen(it)">화면 채우기</button>
                <button class="secondary" (click)="centerOnStage(it)">가운데</button>
              </div>
              <div class="grid2">
                <label>X<input type="number" [ngModel]="it.x" (ngModelChange)="patch(it, 'x', +$event)" /></label>
                <label>Y<input type="number" [ngModel]="it.y" (ngModelChange)="patch(it, 'y', +$event)" /></label>
                <label>너비<input type="number" [ngModel]="it.width" (ngModelChange)="patch(it, 'width', +$event)" /></label>
                <label>높이<input type="number" [ngModel]="it.height" (ngModelChange)="patch(it, 'height', +$event)" /></label>
              </div>

              <label>레이어 순서 (z-index)</label>
              <div class="layer">
                <button class="secondary" (click)="sendToBack(it)">맨 뒤로</button>
                <input type="number" [ngModel]="it.zIndex ?? 1" (ngModelChange)="patch(it, 'zIndex', +$event)" />
                <button class="secondary" (click)="bringToFront(it)">맨 앞으로</button>
              </div>

              <label>불투명도 ({{ (it.opacity ?? 1) * 100 | number: '1.0-0' }}%)</label>
              <input type="range" min="0" max="1" step="0.05" [ngModel]="it.opacity ?? 1" (ngModelChange)="patch(it, 'opacity', +$event)" />

              <label>배경색 (딤·박스용)</label>
              <input [ngModel]="it.background ?? ''" (ngModelChange)="patch(it, 'background', $event)" placeholder="rgba(0,0,0,0.5)" />

              @if (it.kind === 'text') {
                <label>텍스트</label>
                <input [ngModel]="it.text" (ngModelChange)="patch(it, 'text', $event)" />
                <div class="grid2">
                  <label>글자 크기<input type="number" [ngModel]="it.fontSize" (ngModelChange)="patch(it, 'fontSize', +$event)" /></label>
                  <label>줄간격<input type="number" step="0.1" [ngModel]="it.lineHeight ?? 1.2" (ngModelChange)="patch(it, 'lineHeight', +$event)" /></label>
                </div>
                <label>글자색</label>
                <input [ngModel]="it.color" (ngModelChange)="patch(it, 'color', $event)" />
                <div class="grid2">
                  <label>굵기
                    <select [ngModel]="it.fontWeight ?? 400" (ngModelChange)="patch(it, 'fontWeight', +$event)">
                      <option [ngValue]="300">가늘게</option>
                      <option [ngValue]="400">보통</option>
                      <option [ngValue]="600">중간</option>
                      <option [ngValue]="700">굵게</option>
                      <option [ngValue]="800">아주 굵게</option>
                    </select>
                  </label>
                  <label>정렬
                    <select [ngModel]="it.textAlign ?? 'left'" (ngModelChange)="patch(it, 'textAlign', $event)">
                      <option ngValue="left">왼쪽</option>
                      <option ngValue="center">가운데</option>
                      <option ngValue="right">오른쪽</option>
                    </select>
                  </label>
                </div>
              }
            } @else {
              <p class="muted">선택된 항목이 없습니다.</p>
              <p class="muted small">에셋을 클릭하면 추가됩니다. 항목 선택 후 “화면 채우기”로 풀스크린, “＋ 딤 레이어”로 어두운 배경막을 깝니다.</p>
            }
          </div>
        </div>
      </div>
    }
  `,
  styles: [
    `
      .toolbar { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
      .spacer { flex: 1; }
      .save-state { font-size: 12px; color: #1f9d4f; }
      .save-state.dirty { color: #c0392b; font-weight: 600; }
      .bg-bar {
        display: flex; align-items: center; gap: 8px; flex-wrap: wrap;
        margin-top: 10px; padding: 8px 10px; background: #f3f6fc;
        border: 1px solid var(--border); border-radius: 8px;
      }
      .bg-label { font-size: 12px; font-weight: 600; }
      .bg-hex { width: 150px; font-size: 12px; }
      .bg-bar .div { width: 1px; height: 20px; background: var(--border); }
      .bg-bar .hint { font-size: 11px; }
      .editor { display: grid; grid-template-columns: 230px 1fr 260px; gap: 12px; margin-top: 12px; }
      .library, .inspector { background: #fafbfd; padding: 10px; border-radius: 8px; border: 1px solid var(--border); }
      .library h3, .inspector h3 { font-size: 13px; margin: 6px 0 6px; }
      .library-add { display: flex; flex-direction: column; gap: 6px; margin-bottom: 10px; padding-bottom: 10px; border-bottom: 1px solid var(--border); }
      .library-add input, .library-add textarea { font-size: 12px; }
      .file-input { display: none; }
      .library-list { display: flex; flex-direction: column; gap: 6px; max-height: 200px; overflow: auto; }
      .library-item {
        display: flex; align-items: center; gap: 6px; padding: 4px 6px;
        border: 1px solid var(--border); border-radius: 6px; background: #fff;
        cursor: pointer; text-align: left; width: 100%; font: inherit;
      }
      .library-item:hover { border-color: var(--accent); }
      .library-item:focus-visible { outline: 2px solid var(--accent); outline-offset: 1px; }
      .library-item img { width: 36px; height: 24px; object-fit: cover; border-radius: 3px; }
      .library-item .name { font-size: 12px; line-height: 1.2; word-break: break-all; }
      .stage { position: relative; border-radius: 8px; overflow: hidden; }
      .item { position: absolute; background: rgba(255, 255, 255, 0.05); border: 1px solid transparent; cursor: move; overflow: hidden; }
      .item.selected { border-color: var(--accent); box-shadow: 0 0 0 2px rgba(42, 108, 255, 0.4); }
      .item img, .item video { width: 100%; height: 100%; object-fit: cover; pointer-events: none; }
      .item .text { padding: 6px; white-space: pre-wrap; width: 100%; height: 100%; box-sizing: border-box; }
      .handle { position: absolute; right: -4px; bottom: -4px; width: 12px; height: 12px; background: var(--accent); border-radius: 50%; cursor: nwse-resize; z-index: 9999; }
      .inspector label { display: block; font-size: 11px; color: var(--muted); margin: 8px 0 2px; }
      .inspector .grid2 { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; }
      .inspector .grid2 label { margin: 4px 0 0; }
      .inspector input, .inspector select { width: 100%; box-sizing: border-box; }
      .quick { display: flex; gap: 6px; margin-bottom: 4px; }
      .quick button { flex: 1; }
      .layer { display: flex; gap: 4px; align-items: center; }
      .layer input { width: 64px; }
      .small { font-size: 11px; }
      .save-toast {
        display: inline-flex; align-items: center; padding: 4px 10px; margin-left: 6px;
        background: #1f8c4d; color: #fff; border-radius: 12px; font-size: 12px;
        line-height: 1.4; animation: fadeIn 0.15s ease-out;
      }
      .save-toast.error { background: #c0392b; }
      @keyframes fadeIn { from { opacity: 0; transform: translateY(-2px); } to { opacity: 1; transform: translateY(0); } }
    `,
  ],
})
export class ScreenEditPage implements OnInit, OnDestroy {
  readonly api = inject(ApiService);
  private readonly route = inject(ActivatedRoute);
  readonly stage = viewChild<ElementRef<HTMLDivElement>>('stage');
  readonly screen = signal<ScreenView | null>(null);
  readonly assets = signal<AssetView[]>([]);
  readonly components = signal<Array<{ id: number; name: string; kind: string; payload: ScreenLayoutItem | ScreenLayoutItem[] }>>([]);
  readonly selected = signal<ScreenLayoutItem | null>(null);
  readonly dirty = signal(false);
  newTextName = '';
  newTextBody = '';
  readonly textBusy = signal(false);

  readonly saveBusy = signal(false);
  readonly saveMessage = signal<string | null>(null);
  readonly saveError = signal(false);
  private saveMsgTimer: ReturnType<typeof setTimeout> | null = null;
  private autosaveTimer: ReturnType<typeof setTimeout> | null = null;

  private drag: DragState | null = null;
  private dragMoveListener: ((ev: MouseEvent) => void) | null = null;
  private dragUpListener: (() => void) | null = null;

  @HostListener('window:beforeunload', ['$event'])
  onBeforeUnload(ev: BeforeUnloadEvent) {
    if (this.dirty()) {
      ev.preventDefault();
      ev.returnValue = '';
    }
  }

  ngOnInit() {
    const id = Number(this.route.snapshot.paramMap.get('id'));
    this.api.getScreen(id).subscribe((s) => this.screen.set(s));
    this.refreshAssets();
    this.api.listComponents().subscribe((c) => this.components.set(c));
  }

  ngOnDestroy() {
    if (this.autosaveTimer) clearTimeout(this.autosaveTimer);
    if (this.saveMsgTimer) clearTimeout(this.saveMsgTimer);
    this.detachDragListeners();
  }

  private refreshAssets() {
    this.api.listAssets().subscribe((a) => this.assets.set(a));
  }

  /** Items sorted by z-index so editor stacking matches the player. */
  sortedItems(): ScreenLayoutItem[] {
    const s = this.screen();
    if (!s) return [];
    return [...s.layout.items].sort((a, b) => (a.zIndex ?? 1) - (b.zIndex ?? 1));
  }

  bgColor(): string {
    const bg = this.screen()?.layout.background ?? '';
    return /^#[0-9a-fA-F]{6}$/.test(bg) ? bg : '#0c0f1a';
  }

  markDirty() {
    this.dirty.set(true);
    this.scheduleAutosave();
  }

  private scheduleAutosave() {
    if (this.autosaveTimer) clearTimeout(this.autosaveTimer);
    this.autosaveTimer = setTimeout(() => this.save(), 2500);
  }

  onAssetFiles(ev: Event) {
    const s = this.screen();
    if (!s) return;
    const input = ev.target as HTMLInputElement;
    const files = Array.from(input.files ?? []);
    if (!files.length) return;
    let remaining = files.length;
    for (const file of files) {
      this.api.uploadAsset(file, s.storeId ?? undefined).subscribe({
        next: (asset) => {
          this.refreshAssets();
          this.addFromAsset(asset);
          if (--remaining === 0) input.value = '';
        },
        error: () => {
          if (--remaining === 0) input.value = '';
        },
      });
    }
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
    if (ev.target === ev.currentTarget) this.selected.set(null);
  }

  patch<K extends keyof ScreenLayoutItem>(item: ScreenLayoutItem, key: K, value: ScreenLayoutItem[K]) {
    this.patchMany(item, { [key]: value } as Partial<ScreenLayoutItem>);
  }

  private patchMany(item: ScreenLayoutItem, patch: Partial<ScreenLayoutItem>) {
    const s = this.screen();
    if (!s) return;
    const items = s.layout.items.map((i) => (i.id === item.id ? { ...i, ...patch } : i));
    this.screen.set({ ...s, layout: { ...s.layout, items } });
    const updated = items.find((i) => i.id === item.id);
    if (updated) this.selected.set(updated);
    this.markDirty();
  }

  private nextZ(): number {
    const s = this.screen();
    if (!s || !s.layout.items.length) return 1;
    return Math.max(...s.layout.items.map((i) => i.zIndex ?? 1)) + 1;
  }

  private addItem(item: ScreenLayoutItem) {
    const s = this.screen();
    if (!s) return;
    this.screen.set({ ...s, layout: { ...s.layout, items: [...s.layout.items, item] } });
    this.selected.set(item);
    this.markDirty();
  }

  addFromAsset(a: AssetView) {
    const s = this.screen();
    if (!s) return;
    const offset = (s.layout.items.length % 8) * 48; // cascade so items don't fully overlap
    this.addItem({
      id: newId(),
      kind: a.type,
      assetId: a.id,
      x: 80 + offset,
      y: 80 + offset,
      width: a.type === 'text' ? 600 : 400,
      height: a.type === 'text' ? 200 : 300,
      zIndex: this.nextZ(),
      ...(a.type === 'text' ? { text: a.textContent ?? '', fontSize: 36, color: '#ffffff' } : {}),
    });
  }

  addFromComponent(c: { kind: string; payload: ScreenLayoutItem | ScreenLayoutItem[] }) {
    const s = this.screen();
    if (!s) return;
    const items = Array.isArray(c.payload) ? c.payload : [c.payload];
    const cloned = items.map((i) => ({ ...i, id: newId() }));
    this.screen.set({ ...s, layout: { ...s.layout, items: [...s.layout.items, ...cloned] } });
    this.markDirty();
  }

  /** One-click dark scrim over the whole canvas (menu-board "dim"). */
  addDimLayer() {
    const s = this.screen();
    if (!s) return;
    this.addItem({
      id: newId(),
      kind: 'text',
      text: '',
      background: 'rgba(8,10,16,0.55)',
      x: 0,
      y: 0,
      width: s.width,
      height: s.height,
      zIndex: this.nextZ(),
    });
  }

  duplicateSelected() {
    const sel = this.selected();
    if (!sel) return;
    this.addItem({ ...sel, id: newId(), x: sel.x + 40, y: sel.y + 40, zIndex: this.nextZ() });
  }

  fitToScreen(it: ScreenLayoutItem) {
    const s = this.screen();
    if (!s) return;
    this.patchMany(it, { x: 0, y: 0, width: s.width, height: s.height });
  }

  centerOnStage(it: ScreenLayoutItem) {
    const s = this.screen();
    if (!s) return;
    this.patchMany(it, { x: Math.round((s.width - it.width) / 2), y: Math.round((s.height - it.height) / 2) });
  }

  bringToFront(it: ScreenLayoutItem) {
    this.patch(it, 'zIndex', this.nextZ());
  }

  sendToBack(it: ScreenLayoutItem) {
    const s = this.screen();
    if (!s) return;
    const min = Math.min(...s.layout.items.map((i) => i.zIndex ?? 1));
    this.patch(it, 'zIndex', min - 1);
  }

  setScreenBg(value: string) {
    const s = this.screen();
    if (!s) return;
    this.screen.set({ ...s, layout: { ...s.layout, background: value || undefined } });
    this.markDirty();
  }

  removeSelected() {
    const sel = this.selected();
    const s = this.screen();
    if (!sel || !s) return;
    this.screen.set({ ...s, layout: { ...s.layout, items: s.layout.items.filter((i) => i.id !== sel.id) } });
    this.selected.set(null);
    this.markDirty();
  }

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
      return { ...i, width: Math.round(newW), height: Math.round(newH) };
    });
    this.screen.set({ ...s, layout: { ...s.layout, items } });
    const updated = items.find((i) => i.id === this.drag!.itemId);
    if (updated) this.selected.set(updated);
  }

  endDrag() {
    if (this.drag) this.markDirty();
    this.drag = null;
  }

  save() {
    const s = this.screen();
    if (!s) return;
    if (this.autosaveTimer) {
      clearTimeout(this.autosaveTimer);
      this.autosaveTimer = null;
    }
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
          this.dirty.set(false);
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
