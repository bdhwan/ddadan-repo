import { Component, inject, OnInit, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { ApiService, AssetView } from '../api.service';

@Component({
  standalone: true,
  imports: [FormsModule],
  template: `
    <h1>에셋</h1>
    <div class="panel">
      <div class="toolbar">
        <input type="file" (change)="onFile($event)" accept="image/*,video/*" />
        <input [(ngModel)]="textName" placeholder="텍스트 이름" />
        <input [(ngModel)]="textBody" placeholder="텍스트 내용" />
        <button (click)="addText()" [disabled]="!textName.trim() || !textBody.trim()">텍스트 추가</button>
      </div>
      <div class="grid">
        @for (a of assets(); track a.id) {
          <div class="card">
            <div class="preview">
              @switch (a.type) {
                @case ('image') {
                  <img [src]="a.url ?? ''" alt="" />
                }
                @case ('video') {
                  <video [src]="a.url ?? ''" muted></video>
                }
                @default {
                  <div class="text">{{ a.textContent }}</div>
                }
              }
            </div>
            <div class="meta">
              <strong>{{ a.originalName }}</strong>
              <div class="muted small">{{ a.type }} · {{ formatSize(a.sizeBytes) }}</div>
            </div>
            <button class="secondary danger" (click)="remove(a.id)">삭제</button>
          </div>
        } @empty {
          <p class="muted">에셋이 없습니다.</p>
        }
      </div>
    </div>
  `,
  styles: [
    `
      .grid {
        display: grid;
        grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
        gap: 12px;
        margin-top: 12px;
      }
      .card {
        border: 1px solid var(--border);
        border-radius: 8px;
        padding: 8px;
        display: flex;
        flex-direction: column;
        gap: 6px;
      }
      .preview {
        height: 110px;
        background: #f0f3fa;
        border-radius: 6px;
        overflow: hidden;
        display: flex;
        align-items: center;
        justify-content: center;
      }
      .preview img,
      .preview video {
        width: 100%;
        height: 100%;
        object-fit: cover;
      }
      .text {
        padding: 6px;
        font-size: 11px;
        overflow: hidden;
      }
      .small {
        font-size: 11px;
      }
      .danger {
        color: var(--danger);
        border-color: #f0c0c0;
      }
    `,
  ],
})
export class AssetsPage implements OnInit {
  private readonly api = inject(ApiService);
  readonly assets = signal<AssetView[]>([]);
  textName = '';
  textBody = '';

  ngOnInit() {
    this.refresh();
  }

  refresh() {
    this.api.listAssets().subscribe((a) => this.assets.set(a));
  }

  onFile(ev: Event) {
    const input = ev.target as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    this.api.uploadAsset(file).subscribe(() => {
      input.value = '';
      this.refresh();
    });
  }

  addText() {
    this.api.createTextAsset(this.textName.trim(), this.textBody.trim()).subscribe(() => {
      this.textName = '';
      this.textBody = '';
      this.refresh();
    });
  }

  remove(id: number) {
    if (!confirm('삭제할까요?')) return;
    this.api.deleteAsset(id).subscribe(() => this.refresh());
  }

  formatSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  }
}
