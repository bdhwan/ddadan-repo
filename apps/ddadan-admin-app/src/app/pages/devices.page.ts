import { Component, inject, OnInit, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { RouterLink } from '@angular/router';
import { ApiService, DeviceView, MonitorView, ScreenView } from '../api.service';

@Component({
  standalone: true,
  imports: [FormsModule, RouterLink],
  template: `
    <h1>디바이스</h1>
    <div class="panel quick-previews">
      <div class="quick-title">화면 미리보기</div>
      <div class="quick-links">
        @for (id of quickPreviewIds; track id) {
          <a class="preview-chip" [href]="previewUrl(id)" target="_blank" rel="noopener">
            {{ id }}
            <span class="path">{{ previewPath(id) }}</span>
          </a>
        }
      </div>
    </div>
    <div class="panel">
      <div class="toolbar">
        <input [(ngModel)]="hardwareId" placeholder="하드웨어 ID (Pi에서 표시되는 코드)" />
        <input [(ngModel)]="alias" placeholder="별칭" />
        <button
          (click)="register()"
          [disabled]="busy() || !hardwareId.trim() || registerStoreId() == null"
        >
          수동 등록
        </button>
      </div>
      <table class="list">
        <thead>
          <tr>
            <th>이름</th>
            <th>하드웨어 ID</th>
            <th>상태</th>
            <th>화면 할당</th>
            <th>미리보기</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          @for (d of devices(); track d.id) {
            <tr>
              <td>
                <a [routerLink]="['/devices', d.id]">{{ d.name ?? '(이름 없음)' }}</a>
              </td>
              <td><code>{{ d.hardwareId }}</code></td>
              <td>
                <span [class.online]="d.status === 'online'" [class.offline]="d.status !== 'online'">
                  {{ d.status }}
                </span>
              </td>
              <td class="assign-col">
                @for (m of d.monitors; track m.id) {
                  <div class="slot-summary">
                    <span class="slot-label">슬롯 {{ m.slot }}</span>
                    <span>{{ assignmentSummary(m) }}</span>
                  </div>
                } @empty {
                  <span class="muted">—</span>
                }
                <a class="edit-link" [routerLink]="['/devices', d.id]">상세에서 편집 →</a>
              </td>
              <td class="preview-col">
                <a [href]="previewUrl(d.hardwareId)" target="_blank" rel="noopener">{{ previewPath(d.hardwareId) }}</a>
              </td>
              <td><button class="secondary" (click)="unregister(d.id)">등록 해제</button></td>
            </tr>
          } @empty {
            <tr><td colspan="6" class="muted">등록된 디바이스가 없습니다.</td></tr>
          }
        </tbody>
      </table>
    </div>
  `,
  styles: [
    `
      .toolbar {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 10px;
      }
      .online {
        color: #1f9d4f;
        font-weight: 600;
      }
      .offline {
        color: var(--muted);
      }
      .assign-col {
        vertical-align: top;
        min-width: 200px;
      }
      .slot-summary {
        display: flex;
        gap: 8px;
        font-size: 12px;
        margin-bottom: 4px;
      }
      .slot-label {
        font-weight: 600;
        min-width: 52px;
      }
      .edit-link {
        display: inline-block;
        margin-top: 6px;
        font-size: 12px;
      }
      .quick-previews {
        margin-bottom: 14px;
        padding-top: 14px;
        padding-bottom: 14px;
      }
      .quick-title {
        font-size: 13px;
        font-weight: 600;
        margin-bottom: 10px;
      }
      .quick-links {
        display: flex;
        flex-wrap: wrap;
        gap: 10px;
      }
      .preview-chip {
        display: flex;
        flex-direction: column;
        gap: 2px;
        padding: 10px 14px;
        border: 1px solid var(--border);
        border-radius: 8px;
        background: #fafbfd;
        text-decoration: none;
        color: var(--text);
        min-width: 200px;
      }
      .preview-chip:hover {
        border-color: var(--accent);
        background: #eef3ff;
      }
      .preview-chip .path {
        font-size: 11px;
        color: var(--accent);
        font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
      }
      .preview-col {
        font-size: 12px;
        vertical-align: top;
      }
      .preview-col a {
        font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
        word-break: break-all;
      }
    `,
  ],
})
export class DevicesPage implements OnInit {
  private readonly api = inject(ApiService);
  readonly quickPreviewIds = ['display-1', 'display-2', 'display-3'];
  readonly devices = signal<DeviceView[]>([]);
  readonly screens = signal<ScreenView[]>([]);
  readonly registerStoreId = signal<number | null>(null);
  hardwareId = '';
  alias = '';
  readonly busy = signal(false);

  ngOnInit() {
    this.ensureRegisterStore();
    this.refresh();
    setInterval(() => this.refresh(), 15000);
  }

  refresh() {
    this.api.listAllDevices().subscribe((d) => this.devices.set(d));
    this.api.listScreens().subscribe((s) => this.screens.set(s));
  }

  previewPath(hardwareId: string): string {
    return `/preview/${encodeURIComponent(hardwareId)}`;
  }

  previewUrl(hardwareId: string): string {
    return `${window.location.origin}${this.previewPath(hardwareId)}`;
  }

  assignmentSummary(m: MonitorView): string {
    const rot = m.rotationScreenIds ?? [];
    if (rot.length >= 2) {
      const names = rot
        .map((id) => this.screens().find((s) => s.id === id)?.name ?? `#${id}`)
        .join(' → ');
      return `로테이션 ${rot.length}장 (${names})`;
    }
    if (rot.length === 1) {
      const name = this.screens().find((s) => s.id === rot[0])?.name ?? `#${rot[0]}`;
      return name;
    }
    if (m.currentScreenId != null) {
      return this.screens().find((s) => s.id === m.currentScreenId)?.name ?? `#${m.currentScreenId}`;
    }
    return '미할당';
  }

  private ensureRegisterStore() {
    this.api.listStores().subscribe((stores) => {
      if (stores.length > 0) {
        this.registerStoreId.set(stores[0].id);
        return;
      }
      this.api.createStore('Default').subscribe({
        next: (created) => this.registerStoreId.set(created.id),
        error: () => this.registerStoreId.set(null),
      });
    });
  }

  register() {
    const storeId = this.registerStoreId();
    if (storeId == null) return;
    this.busy.set(true);
    this.api
      .registerDevice(storeId, this.hardwareId.trim(), this.alias.trim() || undefined)
      .subscribe({
        next: () => {
          this.hardwareId = '';
          this.alias = '';
          this.busy.set(false);
          this.refresh();
        },
        error: () => this.busy.set(false),
      });
  }

  unregister(id: number) {
    if (!confirm('이 디바이스의 등록을 해제할까요?')) return;
    this.api.unregisterDevice(id).subscribe(() => this.refresh());
  }
}
