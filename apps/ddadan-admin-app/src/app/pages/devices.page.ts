import { SlicePipe } from '@angular/common';
import { Component, inject, OnInit, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { ApiService, DeviceView } from '../api.service';

@Component({
  standalone: true,
  imports: [FormsModule, RouterLink, SlicePipe],
  template: `
    <h1>디바이스</h1>
    <div class="panel">
      <div class="toolbar">
        <input [(ngModel)]="hardwareId" placeholder="하드웨어 ID (Pi에서 표시되는 코드)" />
        <input [(ngModel)]="alias" placeholder="별칭" />
        <button (click)="register()" [disabled]="busy() || !hardwareId.trim()">수동 등록</button>
      </div>
      <table class="list">
        <thead>
          <tr>
            <th>이름</th>
            <th>하드웨어 ID</th>
            <th>상태</th>
            <th>마지막 접속</th>
            <th>모니터</th>
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
              <td>{{ d.monitors.length }}</td>
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
      .online {
        color: #1f9d4f;
        font-weight: 600;
      }
      .offline {
        color: var(--muted);
      }
    `,
  ],
})
export class DevicesPage implements OnInit {
  private readonly api = inject(ApiService);
  private readonly route = inject(ActivatedRoute);
  readonly devices = signal<DeviceView[]>([]);
  storeId = 0;
  hardwareId = '';
  alias = '';
  readonly busy = signal(false);

  ngOnInit() {
    this.storeId = Number(this.route.snapshot.paramMap.get('id'));
    this.refresh();
    setInterval(() => this.refresh(), 15000);
  }

  refresh() {
    this.api.listDevices(this.storeId).subscribe((d) => this.devices.set(d));
  }

  register() {
    this.busy.set(true);
    this.api
      .registerDevice(this.storeId, this.hardwareId.trim(), this.alias.trim() || undefined)
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
