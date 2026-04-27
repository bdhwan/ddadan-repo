import { Component, inject, OnInit, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { ActivatedRoute, Router } from '@angular/router';
import { ApiService, Store } from '../api.service';

@Component({
  standalone: true,
  imports: [FormsModule],
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
        <button (click)="register()" [disabled]="busy() || !selectedStoreId" style="width:100%; margin-top:6px">
          이 매장에 등록
        </button>
        <p class="muted" style="margin-top:12px">
          매장이 없다면 먼저 <a (click)="goToStores()">매장 만들기</a>를 진행하세요.
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
  name = '';
  readonly busy = signal(false);
  readonly error = signal<string | null>(null);

  ngOnInit() {
    const id = this.route.snapshot.queryParamMap.get('deviceId') ?? '';
    this.hardwareId.set(id);
    this.api.listStores().subscribe((s) => this.stores.set(s));
  }

  register() {
    if (!this.selectedStoreId) return;
    this.busy.set(true);
    this.error.set(null);
    this.api.registerDevice(this.selectedStoreId, this.hardwareId(), this.name || undefined).subscribe({
      next: () => {
        this.busy.set(false);
        this.router.navigate(['/stores', this.selectedStoreId, 'devices']);
      },
      error: (err) => {
        this.busy.set(false);
        this.error.set(err?.error?.message ?? err.message);
      },
    });
  }

  goToStores() {
    this.router.navigateByUrl('/stores');
  }
}
