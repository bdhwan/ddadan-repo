import { Component, inject, OnInit, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { ApiService, AssetView } from '../api.service';

@Component({
  standalone: true,
  imports: [FormsModule],
  template: `
    <h1>에셋</h1>
    <div class="panel">
      <div
        class="dropzone"
        [class.drag]="dragOver()"
        (dragover)="onDragOver($event)"
        (dragleave)="dragOver.set(false)"
        (drop)="onDrop($event)"
      >
        <input #fileInput type="file" class="hidden" accept="image/*,video/*" multiple (change)="onFiles($event)" />
        <p>파일을 여기에 드래그하거나</p>
        <button type="button" class="secondary" (click)="fileInput.click()">여러 파일 선택</button>
      </div>
      <div class="toolbar">
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
                  <img [src]="api.absoluteAssetUrl(a.url)" alt="" />
                }
                @case ('video') {
                  <video [src]="api.absoluteAssetUrl(a.url)" muted></video>
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
      .dropzone {
        border: 2px dashed var(--border);
        border-radius: 10px;
        padding: 20px;
        text-align: center;
        margin-bottom: 12px;
        background: #fafbfd;
      }
      .dropzone.drag { border-color: var(--accent); background: #eef3ff; }
      .hidden { display: none; }
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
  readonly api = inject(ApiService);
  readonly assets = signal<AssetView[]>([]);
  readonly dragOver = signal(false);
  textName = '';
  textBody = '';

  ngOnInit() {
    this.refresh();
  }

  refresh() {
    this.api.listAssets().subscribe((a) => this.assets.set(a));
  }

  onDragOver(ev: DragEvent) {
    ev.preventDefault();
    this.dragOver.set(true);
  }

  onDrop(ev: DragEvent) {
    ev.preventDefault();
    this.dragOver.set(false);
    const files = Array.from(ev.dataTransfer?.files ?? []);
    this.uploadFiles(files);
  }

  onFiles(ev: Event) {
    const input = ev.target as HTMLInputElement;
    const files = Array.from(input.files ?? []);
    this.uploadFiles(files);
    input.value = '';
  }

  private uploadFiles(files: File[]) {
    if (!files.length) return;
    let remaining = files.length;
    for (const file of files) {
      this.api.uploadAsset(file).subscribe({
        next: () => {
          if (--remaining === 0) this.refresh();
        },
        error: () => {
          if (--remaining === 0) this.refresh();
        },
      });
    }
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
