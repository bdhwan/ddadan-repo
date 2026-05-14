import { SlicePipe } from '@angular/common';
import { Component, inject, OnInit, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { RouterLink } from '@angular/router';
import { ApiService, DeviceView, MonitorView, ScreenView } from '../api.service';

interface RotForm {
  screenIds: number[];
  intervalSec: number;
  fadeMs: number;
}

@Component({
  standalone: true,
  imports: [FormsModule, RouterLink, SlicePipe],
  template: `
    <h1>디바이스</h1>
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
            <th>마지막 접속</th>
            <th>화면 로테이션</th>
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
              <td>{{ d.lastSeenAt ? (d.lastSeenAt | slice: 0:19) : '-' }}</td>
              <td class="screen-col">
                @for (m of d.monitors; track m.id) {
                  <div class="monitor-rot">
                    <div class="slot">슬롯 {{ m.slot }}</div>
                    <div class="checks">
                      @for (s of screens(); track s.id) {
                        <label class="chk">
                          <input
                            type="checkbox"
                            [checked]="isPicked(m.id, s.id)"
                            (change)="togglePick(m.id, s.id, $any($event.target).checked)"
                          />
                          <span>{{ s.name }}</span>
                        </label>
                      }
                    </div>
                    <div class="rot-opts">
                      <label>
                        간격(초)
                        <input
                          type="number"
                          min="2"
                          step="1"
                          [ngModel]="form(m.id).intervalSec"
                          (ngModelChange)="patchForm(m.id, { intervalSec: +$event })"
                        />
                      </label>
                      <label>
                        페이드(ms)
                        <input
                          type="number"
                          min="200"
                          step="100"
                          [ngModel]="form(m.id).fadeMs"
                          (ngModelChange)="patchForm(m.id, { fadeMs: +$event })"
                        />
                      </label>
                      <button type="button" class="secondary" (click)="applyRotation(m.id)">적용</button>
                    </div>
                    <p class="hint muted">
                      2개 이상 선택 시 크로스 페이드 로테이션. 1개만 선택 시 고정 화면.
                    </p>
                  </div>
                } @empty {
                  <span class="muted">—</span>
                }
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
      .screen-col {
        vertical-align: top;
        min-width: 260px;
        max-width: 420px;
      }
      .monitor-rot {
        margin-bottom: 12px;
        padding-bottom: 10px;
        border-bottom: 1px solid var(--border);
      }
      .monitor-rot:last-child {
        border-bottom: none;
        margin-bottom: 0;
        padding-bottom: 0;
      }
      .slot {
        font-size: 12px;
        font-weight: 600;
        margin-bottom: 6px;
      }
      .checks {
        display: flex;
        flex-direction: column;
        gap: 4px;
        max-height: 140px;
        overflow: auto;
        margin-bottom: 8px;
      }
      .chk {
        display: flex;
        align-items: flex-start;
        gap: 6px;
        font-size: 12px;
        cursor: pointer;
      }
      .rot-opts {
        display: flex;
        flex-wrap: wrap;
        align-items: flex-end;
        gap: 8px;
      }
      .rot-opts label {
        display: flex;
        flex-direction: column;
        gap: 2px;
        font-size: 11px;
      }
      .rot-opts input[type='number'] {
        width: 72px;
      }
      .hint {
        font-size: 11px;
        margin: 6px 0 0;
      }
    `,
  ],
})
export class DevicesPage implements OnInit {
  private readonly api = inject(ApiService);
  readonly devices = signal<DeviceView[]>([]);
  readonly screens = signal<ScreenView[]>([]);
  readonly rotForms = signal<Record<number, RotForm>>({});
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
    this.api.listAllDevices().subscribe((d) => {
      this.devices.set(d);
      this.rotForms.update((prev) => {
        const next = { ...prev };
        for (const dev of d) {
          for (const m of dev.monitors) {
            if (next[m.id] === undefined) {
              next[m.id] = this.initForm(m);
            }
          }
        }
        return next;
      });
    });
    this.api.listScreens().subscribe((s) => this.screens.set(s));
  }

  private initForm(m: MonitorView): RotForm {
    let screenIds: number[] = [];
    const rot = m.rotationScreenIds ?? [];
    if (rot.length >= 2) {
      screenIds = [...rot];
    } else if (m.currentScreenId != null) {
      screenIds = [m.currentScreenId];
    }
    return {
      screenIds,
      intervalSec: Math.max(2, Math.round((m.rotationIntervalMs ?? 10_000) / 1000)),
      fadeMs: Math.max(200, m.rotationFadeMs ?? 800),
    };
  }

  form(monitorId: number): RotForm {
    return this.rotForms()[monitorId] ?? { screenIds: [], intervalSec: 10, fadeMs: 800 };
  }

  isPicked(monitorId: number, screenId: number): boolean {
    return this.form(monitorId).screenIds.includes(screenId);
  }

  togglePick(monitorId: number, screenId: number, checked: boolean) {
    const order = this.screens().map((s) => s.id);
    const f = this.form(monitorId);
    let ids = [...f.screenIds];
    if (checked) {
      if (!ids.includes(screenId)) ids.push(screenId);
    } else {
      ids = ids.filter((id) => id !== screenId);
    }
    ids.sort((a, b) => order.indexOf(a) - order.indexOf(b));
    this.patchForm(monitorId, { screenIds: ids });
  }

  patchForm(monitorId: number, patch: Partial<RotForm>) {
    this.rotForms.update((forms) => ({
      ...forms,
      [monitorId]: { ...this.form(monitorId), ...patch },
    }));
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
        this.api.listAllDevices().subscribe((d) => {
          this.devices.set(d);
          const m = d.flatMap((dev) => dev.monitors).find((x) => x.id === monitorId);
          this.rotForms.update((prev) => {
            const next = { ...prev };
            for (const dev of d) {
              for (const mon of dev.monitors) {
                if (next[mon.id] === undefined) {
                  next[mon.id] = this.initForm(mon);
                }
              }
            }
            if (m) {
              next[monitorId] = this.initForm(m);
            }
            return next;
          });
        });
      });
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
