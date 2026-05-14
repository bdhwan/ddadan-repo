import { Component, inject, OnInit, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { ActivatedRoute, Router, RouterLink } from '@angular/router';
import { ApiService, Store } from '../api.service';

@Component({
  standalone: true,
  imports: [FormsModule, RouterLink],
  template: `
    <div class="wrap">
      <div class="panel" style="width:520px">
        <h2>디바이스 등록</h2>
        <div class="field">
          <label>디바이스 ID (자동 입력됨)</label>
          <input [value]="hardwareId()" readonly />
        </div>
        <div class="field">
          <label>매장 선택</label>
          <select [(ngModel)]="selectedStoreId">
            <option [ngValue]="null" disabled>매장을 선택하세요</option>
            @for (s of stores(); track s.id) {
              <option [ngValue]="s.id">{{ s.name }}</option>
            }
          </select>
        </div>
        <div class="field">
          <label>디바이스 별칭 (선택)</label>
          <input [(ngModel)]="name" placeholder="예: 카운터 위 모니터" />
        </div>
        @if (error()) {
          <div class="error">{{ error() }}</div>
        }
        @if (stores().length === 0) {
          <div class="field">
            <label>새 매장 만들기</label>
            <input [(ngModel)]="newStoreName" placeholder="매장 이름" />
            <button
              type="button"
              class="secondary"
              style="width:100%; margin-top:8px"
              (click)="addStore()"
              [disabled]="storeBusy() || !newStoreName.trim()"
            >
              매장 추가 후 계속
            </button>
          </div>
        }
        <button (click)="register()" [disabled]="busy() || !selectedStoreId" style="width:100%; margin-top:6px">
          이 매장에 등록
        </button>
        <p class="muted" style="margin-top:12px">
          매장은 <a routerLink="/devices">디바이스</a> 화면에서도 추가할 수 있습니다.
        </p>
      </div>
    </div>
  `,
  styles: [
    `
      .wrap {
        min-height: 100vh;
        display: flex;
        align-items: center;
        justify-content: center;
      }
    `,
  ],
})
export class RegisterPage implements OnInit {
  private readonly api = inject(ApiService);
  private readonly route = inject(ActivatedRoute);
  private readonly router = inject(Router);
  readonly hardwareId = signal('');
  readonly stores = signal<Store[]>([]);
  selectedStoreId: number | null = null;
  newStoreName = '';
  name = '';
  readonly busy = signal(false);
  readonly storeBusy = signal(false);
  readonly error = signal<string | null>(null);

  ngOnInit() {
    const id = this.route.snapshot.queryParamMap.get('deviceId') ?? '';
    this.hardwareId.set(id);
    this.api.listStores().subscribe((s) => {
      this.stores.set(s);
      if (s.length && this.selectedStoreId == null) {
        this.selectedStoreId = s[0].id;
      }
    });
  }

  addStore() {
    const name = this.newStoreName.trim();
    if (!name) return;
    this.storeBusy.set(true);
    this.error.set(null);
    this.api.createStore(name).subscribe({
      next: (created) => {
        this.newStoreName = '';
        this.storeBusy.set(false);
        this.selectedStoreId = created.id;
        this.api.listStores().subscribe((s) => this.stores.set(s));
      },
      error: (err) => {
        this.storeBusy.set(false);
        this.error.set(err?.error?.message ?? err.message);
      },
    });
  }

  register() {
    if (!this.selectedStoreId) return;
    this.busy.set(true);
    this.error.set(null);
    this.api.registerDevice(this.selectedStoreId, this.hardwareId(), this.name || undefined).subscribe({
      next: () => {
        this.busy.set(false);
        this.router.navigate(['/devices']);
      },
      error: (err) => {
        this.busy.set(false);
        this.error.set(err?.error?.message ?? err.message);
      },
    });
  }
}
