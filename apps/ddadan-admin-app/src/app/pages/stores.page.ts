import { Component, inject, OnInit, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { RouterLink } from '@angular/router';
import { ApiService, Store } from '../api.service';

@Component({
  standalone: true,
  imports: [FormsModule, RouterLink],
  template: `
    <h1>매장</h1>
    <div class="panel">
      <div class="toolbar">
        <input [(ngModel)]="newName" placeholder="새 매장 이름" />
        <button (click)="create()" [disabled]="!newName.trim() || busy()">매장 추가</button>
      </div>
      <table class="list">
        <thead>
          <tr><th>이름</th><th>업종</th><th>타임존</th><th></th></tr>
        </thead>
        <tbody>
          @for (s of stores(); track s.id) {
            <tr>
              <td><a [routerLink]="['/stores', s.id, 'devices']">{{ s.name }}</a></td>
              <td>{{ s.businessType ?? '-' }}</td>
              <td>{{ s.timezone }}</td>
              <td><button class="secondary" (click)="remove(s.id)">삭제</button></td>
            </tr>
          } @empty {
            <tr><td colspan="4" class="muted">매장이 없습니다.</td></tr>
          }
        </tbody>
      </table>
    </div>
  `,
})
export class StoresPage implements OnInit {
  private readonly api = inject(ApiService);
  readonly stores = signal<Store[]>([]);
  newName = '';
  readonly busy = signal(false);

  ngOnInit() {
    this.refresh();
  }

  refresh() {
    this.api.listStores().subscribe((s) => this.stores.set(s));
  }

  create() {
    if (!this.newName.trim()) return;
    this.busy.set(true);
    this.api.createStore(this.newName.trim()).subscribe({
      next: () => {
        this.newName = '';
        this.busy.set(false);
        this.refresh();
      },
      error: () => this.busy.set(false),
    });
  }

  remove(id: number) {
    if (!confirm('이 매장과 모든 디바이스가 삭제됩니다. 계속할까요?')) return;
    this.api.deleteStore(id).subscribe(() => this.refresh());
  }
}
