import { Component, inject, OnInit, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { Router, RouterLink } from '@angular/router';
import { ApiService, ScreenView } from '../api.service';

@Component({
  standalone: true,
  imports: [FormsModule, RouterLink],
  template: `
    <h1>화면</h1>
    <div class="panel">
      <div class="toolbar">
        <input [(ngModel)]="newName" placeholder="새 화면 이름" />
        <input type="number" [(ngModel)]="width" placeholder="너비" style="width:100px" />
        <input type="number" [(ngModel)]="height" placeholder="높이" style="width:100px" />
        <button (click)="create()" [disabled]="!newName.trim()">화면 만들기</button>
      </div>
      <table class="list">
        <thead>
          <tr><th>이름</th><th>해상도</th><th>항목</th><th></th></tr>
        </thead>
        <tbody>
          @for (s of screens(); track s.id) {
            <tr>
              <td><a [routerLink]="['/screens', s.id]">{{ s.name }}</a></td>
              <td>{{ s.width }} × {{ s.height }}</td>
              <td class="muted">{{ s.layout.items.length }}개</td>
              <td class="actions">
                <button class="secondary" (click)="duplicate(s)">복제</button>
                <button class="secondary danger" (click)="remove(s.id)">삭제</button>
              </td>
            </tr>
          } @empty {
            <tr><td colspan="4" class="muted">화면이 없습니다.</td></tr>
          }
        </tbody>
      </table>
    </div>
  `,
  styles: [
    `
      .actions { display: flex; gap: 6px; }
    `,
  ],
})
export class ScreensPage implements OnInit {
  private readonly api = inject(ApiService);
  private readonly router = inject(Router);
  readonly screens = signal<ScreenView[]>([]);
  newName = '';
  width = 1920;
  height = 1080;

  ngOnInit() {
    this.refresh();
  }

  refresh() {
    this.api.listScreens().subscribe((s) => this.screens.set(s));
  }

  create() {
    this.api
      .createScreen({
        name: this.newName.trim(),
        width: this.width,
        height: this.height,
        layout: { items: [] },
      })
      .subscribe((created) => {
        this.newName = '';
        this.router.navigate(['/screens', created.id]);
      });
  }

  duplicate(source: ScreenView) {
    const name = prompt('복제 화면 이름', `${source.name} (복사)`);
    if (!name?.trim()) return;
    this.api
      .createScreen({
        name: name.trim(),
        width: source.width,
        height: source.height,
        storeId: source.storeId ?? undefined,
        layout: structuredClone(source.layout),
      })
      .subscribe((created) => this.router.navigate(['/screens', created.id]));
  }

  remove(id: number) {
    if (!confirm('삭제할까요?')) return;
    this.api.deleteScreen(id).subscribe(() => this.refresh());
  }
}
