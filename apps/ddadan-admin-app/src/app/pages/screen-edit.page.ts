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
import { isMenuLine, snapToGrid, textAlignStyle } from '../screen-utils';
import { HasUnsavedChanges } from '../unsaved-changes.guard';

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
          <input [(ngModel)]="s.name" (ngModelChange)="onNameChange()" />
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
          <label class="snap-toggle">
            <input type="checkbox" [ngModel]="snapGrid()" (ngModelChange)="snapGrid.set($event)" />
            8px 스냅
          </label>
          <label class="zoom-label">
            줌
            <input type="range" min="0.35" max="1.5" step="0.05" [ngModel]="stageZoom()" (ngModelChange)="stageZoom.set(+$event)" />
          </label>
          <button class="secondary" (click)="undo()" [disabled]="!canUndo()">↶</button>
          <button class="secondary" (click)="redo()" [disabled]="!canRedo()">↷</button>
          <button class="secondary" (click)="previewOpen.set(true)">미리보기</button>
          @if (selectedIds().size >= 2) {
            <button class="secondary" (click)="alignSelected('left')">←</button>
            <button class="secondary" (click)="alignSelected('centerX')">↔</button>
            <button class="secondary" (click)="alignSelected('right')">→</button>
            <button class="secondary" (click)="alignSelected('top')">↑</button>
            <button class="secondary" (click)="alignSelected('centerY')">↕</button>
            <button class="secondary" (click)="alignSelected('bottom')">↓</button>
            @if (selectedIds().size >= 3) {
              <button class="secondary" (click)="distributeSelected('h')">H분배</button>
              <button class="secondary" (click)="distributeSelected('v')">V분배</button>
            }
          }
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
            <button class="secondary" (click)="addGradientDim()">＋ 그라데이션 딤</button>
            <button class="secondary" (click)="addMenuLine()">＋ 메뉴 줄</button>
          <select class="bg-asset" [ngModel]="bgAssetId()" (ngModelChange)="setScreenBgFromAsset(+$event || 0)">
            <option [ngValue]="0">배경 이미지 선택…</option>
            @for (a of imageAssets(); track a.id) {
              <option [ngValue]="a.id">{{ a.originalName }}</option>
            }
          </select>
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

          <div class="stage-wrap">
          <div
            #stage
            class="stage"
            [style.aspect-ratio]="aspectRatio()"
            [style.background]="s.layout.background || '#1c2436'"
            [style.transform]="'scale(' + stageZoom() + ')'"
            [style.transform-origin]="'top left'"
            (click)="clearSelection($event)"
          >
            @for (item of sortedItems(); track item.id) {
              <div
                class="item"
                [class.selected]="isSelected(item)"
                [style.left.%]="(item.x / s.width) * 100"
                [style.top.%]="(item.y / s.height) * 100"
                [style.width.%]="(item.width / s.width) * 100"
                [style.height.%]="(item.height / s.height) * 100"
                [style.background]="item.background ?? null"
                [style.color]="item.color ?? null"
                [style.font-size.px]="item.fontUnit === 'vh' ? ((item.fontSize ?? 0) * s.height) / 100 : item.fontSize ?? null"
                [style.font-weight]="item.fontWeight ?? null"
                [style.text-align]="textAlignStyle(item)"
                [style.line-height]="item.lineHeight ?? null"
                [style.opacity]="item.opacity ?? null"
                [style.z-index]="item.zIndex ?? 1"
                (click)="select($event, item)"
                (mousedown)="startDrag($event, item, 'move')"
              >
                @switch (item.kind) {
                  @case ('image') { <img [src]="urlOf(item)" alt="" /> }
                  @case ('video') { <video [src]="urlOf(item)" muted autoplay loop></video> }
                  @case ('text') {
                    @if (isMenuLine(item)) {
                      <div class="text menu-line">
                        <span class="label">{{ item.text }}</span>
                        <span class="dots"></span>
                        <span class="price">{{ item.textSecondary }}</span>
                      </div>
                    } @else {
                      <div class="text">{{ item.text }}</div>
                    }
                  }
                }
                <div class="handle" (mousedown)="startDrag($event, item, 'resize')"></div>
              </div>
            }
          </div>
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
                <label>텍스트 유형</label>
                <select [ngModel]="it.textVariant ?? 'plain'" (ngModelChange)="setTextVariant(it, $event)">
                  <option ngValue="plain">일반 텍스트</option>
                  <option ngValue="menuLine">메뉴 줄 (이름 ··· 가격)</option>
                </select>
                @if (isMenuLine(it)) {
                  <label>메뉴명</label>
                  <input [ngModel]="it.text" (ngModelChange)="patch(it, 'text', $event)" />
                  <label>가격</label>
                  <input [ngModel]="it.textSecondary ?? ''" (ngModelChange)="patch(it, 'textSecondary', $event)" />
                } @else {
                  <label>텍스트</label>
                  <input [ngModel]="it.text" (ngModelChange)="patch(it, 'text', $event)" />
                }
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

    @if (previewOpen()) {
      <div class="preview-modal" (click)="previewOpen.set(false)">
        <div class="preview-box" (click)="$event.stopPropagation()">
          <div class="preview-head">
            <strong>실사이즈 미리보기</strong>
            <button class="secondary" (click)="previewOpen.set(false)">닫기</button>
          </div>
          @if (screen(); as ps) {
            <div
              class="preview-stage"
              [style.width.px]="ps.width"
              [style.height.px]="ps.height"
              [style.background]="ps.layout.background || '#1c2436'"
            >
              @for (item of sortedItems(); track item.id) {
                <div
                  class="prev-item"
                  [style.left.px]="item.x"
                  [style.top.px]="item.y"
                  [style.width.px]="item.width"
                  [style.height.px]="item.height"
                  [style.background]="item.background ?? null"
                  [style.color]="item.color ?? null"
                  [style.font-size.px]="item.fontUnit === 'vh' ? ((item.fontSize ?? 0) * ps.height) / 100 : item.fontSize ?? null"
                  [style.font-weight]="item.fontWeight ?? null"
                  [style.text-align]="textAlignStyle(item)"
                  [style.line-height]="item.lineHeight ?? null"
                  [style.opacity]="item.opacity ?? null"
                  [style.z-index]="item.zIndex ?? 1"
                >
                  @switch (item.kind) {
                    @case ('image') { <img [src]="urlOf(item)" alt="" /> }
                    @case ('video') { <video [src]="urlOf(item)" muted autoplay loop></video> }
                    @case ('text') {
                      @if (isMenuLine(item)) {
                        <div class="text menu-line">
                          <span class="label">{{ item.text }}</span>
                          <span class="dots"></span>
                          <span class="price">{{ item.textSecondary }}</span>
                        </div>
                      } @else {
                        <div class="text">{{ item.text }}</div>
                      }
                    }
                  }
                </div>
              }
            </div>
          }
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
      .bg-asset { width: 140px; font-size: 12px; }
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
      .stage-wrap { overflow: auto; max-height: min(72vh, 820px); border-radius: 8px; border: 1px solid var(--border); background: #e8ebf2; padding: 8px; }
      .stage { position: relative; border-radius: 8px; overflow: hidden; }
      .item { position: absolute; background: rgba(255, 255, 255, 0.05); border: 1px solid transparent; cursor: move; overflow: hidden; }
      .item.selected { border-color: var(--accent); box-shadow: 0 0 0 2px rgba(42, 108, 255, 0.4); }
      .item img, .item video { width: 100%; height: 100%; object-fit: cover; pointer-events: none; }
      .item .text { padding: 6px; white-space: pre-wrap; width: 100%; height: 100%; box-sizing: border-box; display: flex; align-items: center; }
      .item .text.menu-line { align-items: baseline; gap: 6px; padding: 4px 8px; }
      .item .menu-line .label { flex: 0 1 auto; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
      .item .menu-line .dots { flex: 1 1 auto; border-bottom: 1px dotted currentColor; opacity: 0.45; min-width: 12px; margin-bottom: 0.2em; }
      .item .menu-line .price { flex: 0 0 auto; white-space: nowrap; }
      .snap-toggle, .zoom-label { font-size: 11px; display: inline-flex; align-items: center; gap: 4px; color: var(--muted); }
      .zoom-label input { width: 80px; }
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
      .preview-modal {
        position: fixed; inset: 0; z-index: 1000; background: rgba(10, 12, 18, 0.72);
        display: flex; align-items: center; justify-content: center; padding: 24px;
      }
      .preview-box { background: #fff; border-radius: 12px; max-width: 96vw; max-height: 92vh; display: flex; flex-direction: column; overflow: hidden; }
      .preview-head { display: flex; justify-content: space-between; align-items: center; padding: 12px 16px; border-bottom: 1px solid var(--border); }
      .preview-stage { position: relative; overflow: auto; margin: 12px; flex: 1; transform-origin: top left; }
      .preview-stage .prev-item { position: absolute; overflow: hidden; }
      .preview-stage img, .preview-stage video { width: 100%; height: 100%; object-fit: cover; }
      .preview-stage .text { width: 100%; height: 100%; display: flex; align-items: center; padding: 8px; box-sizing: border-box; }
      .preview-stage .text.menu-line { align-items: baseline; gap: 8px; }
      .preview-stage .menu-line .label { flex: 0 1 auto; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
      .preview-stage .menu-line .dots { flex: 1 1 auto; border-bottom: 1px dotted currentColor; opacity: 0.45; min-width: 16px; margin-bottom: 0.2em; }
      .preview-stage .menu-line .price { flex: 0 0 auto; white-space: nowrap; }
    `,
  ],
})
export class ScreenEditPage implements OnInit, OnDestroy, HasUnsavedChanges {
  readonly api = inject(ApiService);
  private readonly route = inject(ActivatedRoute);
  readonly stage = viewChild<ElementRef<HTMLDivElement>>('stage');
  readonly screen = signal<ScreenView | null>(null);
  readonly assets = signal<AssetView[]>([]);
  readonly components = signal<Array<{ id: number; name: string; kind: string; payload: ScreenLayoutItem | ScreenLayoutItem[] }>>([]);
  readonly selected = signal<ScreenLayoutItem | null>(null);
  readonly selectedIds = signal<Set<string>>(new Set());
  readonly stageZoom = signal(1);
  readonly snapGrid = signal(true);
  readonly previewOpen = signal(false);
  readonly bgAssetId = signal(0);
  readonly dirty = signal(false);
  newTextName = '';
  newTextBody = '';
  readonly textBusy = signal(false);

  readonly saveBusy = signal(false);
  readonly saveMessage = signal<string | null>(null);
  readonly saveError = signal(false);
  private saveMsgTimer: ReturnType<typeof setTimeout> | null = null;
  private autosaveTimer: ReturnType<typeof setTimeout> | null = null;
  private history: ScreenView[] = [];
  private historyIndex = -1;
  private applyingHistory = false;

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
    this.api.getScreen(id).subscribe((s) => {
      this.screen.set(s);
      this.pushHistory(s);
    });
    this.refreshAssets();
    this.api.listComponents().subscribe((c) => this.components.set(c));
  }

  hasUnsavedChanges(): boolean {
    return this.dirty();
  }

  onNameChange() {
    this.commitState();
    this.markDirty();
  }

  isMenuLine = isMenuLine;
  textAlignStyle = textAlignStyle;

  imageAssets(): AssetView[] {
    return this.assets().filter((a) => a.type === 'image');
  }

  isSelected(item: ScreenLayoutItem): boolean {
    return this.selectedIds().has(item.id);
  }

  private pushHistory(s: ScreenView) {
    const snap = structuredClone(s);
    this.history = [snap];
    this.historyIndex = 0;
  }

  canUndo(): boolean {
    return this.historyIndex > 0;
  }

  canRedo(): boolean {
    return this.historyIndex < this.history.length - 1;
  }

  undo() {
    if (!this.canUndo()) return;
    this.historyIndex--;
    this.applyHistory(this.history[this.historyIndex]);
  }

  redo() {
    if (!this.canRedo()) return;
    this.historyIndex++;
    this.applyHistory(this.history[this.historyIndex]);
  }

  private applyHistory(s: ScreenView) {
    this.applyingHistory = true;
    this.screen.set(structuredClone(s));
    this.selected.set(null);
    this.selectedIds.set(new Set());
    this.dirty.set(true);
    this.applyingHistory = false;
    this.scheduleAutosave();
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

  private commitState() {
    const s = this.screen();
    if (!s || this.applyingHistory) return;
    const snap = structuredClone(s);
    this.history = this.history.slice(0, this.historyIndex + 1);
    this.history.push(snap);
    if (this.history.length > 40) {
      this.history.shift();
    } else {
      this.historyIndex++;
    }
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
    const multi = ev.metaKey || ev.ctrlKey || ev.shiftKey;
    if (multi) {
      const next = new Set(this.selectedIds());
      if (next.has(item.id)) next.delete(item.id);
      else next.add(item.id);
      this.selectedIds.set(next);
      this.selected.set(item);
    } else {
      this.selectedIds.set(new Set([item.id]));
      this.selected.set(item);
    }
  }

  clearSelection(ev: MouseEvent) {
    if (ev.target === ev.currentTarget) {
      this.selected.set(null);
      this.selectedIds.set(new Set());
    }
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
    this.commitState();
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
    this.selectedIds.set(new Set([item.id]));
    this.commitState();
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
    this.commitState();
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

  addGradientDim() {
    const s = this.screen();
    if (!s) return;
    this.addItem({
      id: newId(),
      kind: 'text',
      text: '',
      background: 'linear-gradient(to top, rgba(8,10,16,0.78) 0%, rgba(8,10,16,0.35) 45%, rgba(8,10,16,0.08) 100%)',
      x: 0,
      y: 0,
      width: s.width,
      height: s.height,
      zIndex: this.nextZ(),
    });
  }

  addMenuLine() {
    const s = this.screen();
    if (!s) return;
    const y = 120 + (s.layout.items.length % 12) * 44;
    this.addItem({
      id: newId(),
      kind: 'text',
      textVariant: 'menuLine',
      text: '메뉴명',
      textSecondary: '₩5,500',
      textAlign: 'left',
      fontSize: 32,
      color: '#ffffff',
      x: 80,
      y,
      width: s.width - 160,
      height: 48,
      zIndex: this.nextZ(),
    });
  }

  setScreenBgFromAsset(assetId: number) {
    if (!assetId) return;
    const asset = this.assets().find((a) => a.id === assetId);
    if (!asset?.url) return;
    const url = this.api.absoluteAssetUrl(asset.url);
    this.setScreenBg(`url("${url}") center/cover no-repeat`);
    this.bgAssetId.set(assetId);
  }

  setTextVariant(item: ScreenLayoutItem, variant: 'plain' | 'menuLine') {
    if (variant === 'menuLine') {
      this.patchMany(item, {
        textVariant: 'menuLine',
        textAlign: 'left',
        textSecondary: item.textSecondary ?? '₩0',
      });
    } else {
      this.patchMany(item, { textVariant: 'plain' });
    }
  }

  alignSelected(mode: 'left' | 'right' | 'top' | 'bottom' | 'centerX' | 'centerY') {
    const s = this.screen();
    if (!s) return;
    const ids = this.selectedIds();
    const picked = s.layout.items.filter((i) => ids.has(i.id));
    if (picked.length < 2) return;
    const patchMap = new Map<string, Partial<ScreenLayoutItem>>();
    if (mode === 'left') {
      const min = Math.min(...picked.map((i) => i.x));
      for (const i of picked) patchMap.set(i.id, { x: min });
    } else if (mode === 'right') {
      const max = Math.max(...picked.map((i) => i.x + i.width));
      for (const i of picked) patchMap.set(i.id, { x: max - i.width });
    } else if (mode === 'top') {
      const min = Math.min(...picked.map((i) => i.y));
      for (const i of picked) patchMap.set(i.id, { y: min });
    } else if (mode === 'bottom') {
      const max = Math.max(...picked.map((i) => i.y + i.height));
      for (const i of picked) patchMap.set(i.id, { y: max - i.height });
    } else if (mode === 'centerX') {
      const cx = picked.reduce((sum, i) => sum + i.x + i.width / 2, 0) / picked.length;
      for (const i of picked) patchMap.set(i.id, { x: Math.round(cx - i.width / 2) });
    } else if (mode === 'centerY') {
      const cy = picked.reduce((sum, i) => sum + i.y + i.height / 2, 0) / picked.length;
      for (const i of picked) patchMap.set(i.id, { y: Math.round(cy - i.height / 2) });
    }
    this.applyBulkPatch(patchMap);
  }

  distributeSelected(axis: 'h' | 'v') {
    const s = this.screen();
    if (!s) return;
    const ids = this.selectedIds();
    const picked = s.layout.items.filter((i) => ids.has(i.id));
    if (picked.length < 3) return;
    const patchMap = new Map<string, Partial<ScreenLayoutItem>>();
    if (axis === 'h') {
      const sorted = [...picked].sort((a, b) => a.x - b.x);
      const left = sorted[0].x;
      const right = sorted[sorted.length - 1].x + sorted[sorted.length - 1].width;
      const totalW = sorted.reduce((sum, i) => sum + i.width, 0);
      const gap = (right - left - totalW) / (sorted.length - 1);
      let cursor = left;
      for (const i of sorted) {
        patchMap.set(i.id, { x: Math.round(cursor) });
        cursor += i.width + gap;
      }
    } else {
      const sorted = [...picked].sort((a, b) => a.y - b.y);
      const top = sorted[0].y;
      const bottom = sorted[sorted.length - 1].y + sorted[sorted.length - 1].height;
      const totalH = sorted.reduce((sum, i) => sum + i.height, 0);
      const gap = (bottom - top - totalH) / (sorted.length - 1);
      let cursor = top;
      for (const i of sorted) {
        patchMap.set(i.id, { y: Math.round(cursor) });
        cursor += i.height + gap;
      }
    }
    this.applyBulkPatch(patchMap);
  }

  private applyBulkPatch(patchMap: Map<string, Partial<ScreenLayoutItem>>) {
    const s = this.screen();
    if (!s) return;
    const items = s.layout.items.map((i) => (patchMap.has(i.id) ? { ...i, ...patchMap.get(i.id)! } : i));
    this.screen.set({ ...s, layout: { ...s.layout, items } });
    const sel = this.selected();
    if (sel) {
      const updated = items.find((i) => i.id === sel.id);
      if (updated) this.selected.set(updated);
    }
    this.commitState();
    this.markDirty();
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
    this.commitState();
    this.markDirty();
  }

  removeSelected() {
    const sel = this.selected();
    const s = this.screen();
    if (!sel || !s) return;
    this.screen.set({ ...s, layout: { ...s.layout, items: s.layout.items.filter((i) => i.id !== sel.id) } });
    this.selected.set(null);
    this.selectedIds.set(new Set());
    this.commitState();
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
        let x = Math.round(this.drag!.startItemX + dx);
        let y = Math.round(this.drag!.startItemY + dy);
        if (this.snapGrid()) {
          x = snapToGrid(x);
          y = snapToGrid(y);
        }
        return { ...i, x, y };
      }
      const startW = this.drag!.startItemW;
      const startH = this.drag!.startItemH;
      let newW = Math.max(40, startW + dx);
      let newH = Math.max(40, startH + dy);
      if (this.snapGrid()) {
        newW = Math.max(40, snapToGrid(newW));
        newH = Math.max(40, snapToGrid(newH));
      }
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
    if (this.drag) {
      this.commitState();
      this.markDirty();
    }
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
