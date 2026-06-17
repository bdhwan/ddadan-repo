import { Component, computed, inject, input, output, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { ApiService, AssetView, MonitorView, ScreenView } from '../api.service';
import { screenThumbnailUrl } from '../screen-utils';

interface RotForm {
  screenIds: number[];
  intervalSec: number;
  fadeMs: number;
}

@Component({
  standalone: true,
  imports: [FormsModule],
  selector: 'app-monitor-rotation-panel',
  template: `
    <div class="rot-panel" (mousedown)="$event.stopPropagation()">
      <div class="pick-grid">
        @for (s of screens(); track s.id) {
          <label class="pick" [class.on]="isPicked(s.id)">
            <input type="checkbox" [checked]="isPicked(s.id)" (change)="toggle(s.id, $any($event.target).checked)" />
            <div class="thumb" [style.background]="s.layout.background ?? '#1c2436'">
              @if (thumb(s); as u) {
                <img [src]="u" alt="" />
              } @else {
                <span class="no-thumb">{{ s.name.slice(0, 1) }}</span>
              }
            </div>
            <span class="name">{{ s.name }}</span>
          </label>
        }
      </div>

      @if (form().screenIds.length) {
        <div class="order">
          <div class="order-title">재생 순서 (드래그로 변경)</div>
          @for (id of form().screenIds; track id; let i = $index) {
            <div
              class="order-row"
              draggable="true"
              (dragstart)="onDragStart(i)"
              (dragover)="onDragOver($event, i)"
              (drop)="onDrop(i)"
            >
              <span class="grip">⋮⋮</span>
              @if (thumbById(id); as u) {
                <img class="mini" [src]="u" alt="" />
              }
              <span class="label">{{ nameOf(id) }}</span>
              <button type="button" class="icon secondary" (click)="move(i, -1)" [disabled]="i === 0">↑</button>
              <button type="button" class="icon secondary" (click)="move(i, 1)" [disabled]="i === form().screenIds.length - 1">↓</button>
            </div>
          }
        </div>
      }

      <div class="opts">
        <label>
          간격(초)
          <input type="number" min="2" step="1" [ngModel]="form().intervalSec" (ngModelChange)="patch({ intervalSec: +$event })" />
        </label>
        <label>
          페이드(ms)
          <input type="number" min="200" step="100" [ngModel]="form().fadeMs" (ngModelChange)="patch({ fadeMs: +$event })" />
        </label>
        <button type="button" (click)="apply.emit()">적용</button>
      </div>
      <p class="hint muted">
        {{ form().screenIds.length >= 2 ? '크로스 페이드 로테이션' : form().screenIds.length === 1 ? '고정 화면' : '화면을 선택하세요' }}
      </p>
    </div>
  `,
  styles: [
    `
      .rot-panel { font-size: 11px; }
      .pick-grid {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(72px, 1fr));
        gap: 6px;
        max-height: 120px;
        overflow: auto;
        margin-bottom: 8px;
      }
      .pick {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 3px;
        padding: 4px;
        border: 1px solid rgba(255, 255, 255, 0.15);
        border-radius: 6px;
        cursor: pointer;
        background: rgba(0, 0, 0, 0.2);
      }
      .pick.on { border-color: #6ea8ff; background: rgba(42, 108, 255, 0.15); }
      .pick input { display: none; }
      .thumb {
        width: 100%;
        aspect-ratio: 16 / 10;
        border-radius: 4px;
        overflow: hidden;
        display: flex;
        align-items: center;
        justify-content: center;
      }
      .thumb img { width: 100%; height: 100%; object-fit: cover; }
      .no-thumb { opacity: 0.5; font-weight: 700; }
      .name { text-align: center; line-height: 1.1; word-break: break-all; }
      .order { margin-bottom: 8px; }
      .order-title { font-weight: 600; margin-bottom: 4px; }
      .order-row {
        display: flex;
        align-items: center;
        gap: 4px;
        padding: 3px 4px;
        background: rgba(0, 0, 0, 0.25);
        border-radius: 4px;
        margin-bottom: 3px;
      }
      .grip { opacity: 0.45; cursor: grab; user-select: none; }
      .mini { width: 28px; height: 18px; object-fit: cover; border-radius: 2px; }
      .label { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
      .icon { padding: 2px 6px; font-size: 10px; min-width: 24px; }
      .opts { display: flex; flex-wrap: wrap; gap: 6px; align-items: flex-end; }
      .opts label { display: flex; flex-direction: column; gap: 2px; }
      .opts input[type='number'] { width: 64px; }
      .hint { margin: 6px 0 0; font-size: 10px; }
    `,
  ],
})
export class MonitorRotationPanelComponent {
  readonly api = inject(ApiService);
  readonly monitor = input.required<MonitorView>();
  readonly screens = input.required<ScreenView[]>();
  readonly assets = input.required<AssetView[]>();
  readonly form = input.required<RotForm>();
  readonly formChange = output<RotForm>();
  readonly apply = output<void>();

  private dragFrom = signal<number | null>(null);

  private readonly screenMap = computed(() => {
    const map = new Map<number, ScreenView>();
    for (const s of this.screens()) map.set(s.id, s);
    return map;
  });

  isPicked(id: number): boolean {
    return this.form().screenIds.includes(id);
  }

  toggle(id: number, checked: boolean) {
    const f = this.form();
    let ids = [...f.screenIds];
    if (checked) {
      if (!ids.includes(id)) ids.push(id);
    } else {
      ids = ids.filter((x) => x !== id);
    }
    this.formChange.emit({ ...f, screenIds: ids });
  }

  patch(partial: Partial<RotForm>) {
    this.formChange.emit({ ...this.form(), ...partial });
  }

  nameOf(id: number): string {
    return this.screenMap().get(id)?.name ?? `#${id}`;
  }

  thumb(s: ScreenView): string | null {
    return screenThumbnailUrl(s, this.assets(), (u) => this.api.absoluteAssetUrl(u));
  }

  thumbById(id: number): string | null {
    const s = this.screenMap().get(id);
    return s ? this.thumb(s) : null;
  }

  move(index: number, delta: number) {
    const f = this.form();
    const ids = [...f.screenIds];
    const next = index + delta;
    if (next < 0 || next >= ids.length) return;
    [ids[index], ids[next]] = [ids[next], ids[index]];
    this.formChange.emit({ ...f, screenIds: ids });
  }

  onDragStart(index: number) {
    this.dragFrom.set(index);
  }

  onDragOver(ev: DragEvent, index: number) {
    ev.preventDefault();
    const from = this.dragFrom();
    if (from === null || from === index) return;
  }

  onDrop(index: number) {
    const from = this.dragFrom();
    this.dragFrom.set(null);
    if (from === null || from === index) return;
    const f = this.form();
    const ids = [...f.screenIds];
    const [item] = ids.splice(from, 1);
    ids.splice(index, 0, item);
    this.formChange.emit({ ...f, screenIds: ids });
  }
}

export type { RotForm };
