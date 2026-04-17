import { Component, computed, signal } from '@angular/core';

type MediaType = 'image' | 'video';

interface SignageItem {
  id: string;
  type: MediaType;
  title: string;
  src: string;
  duration?: number;
}

@Component({
  selector: 'app-root',
  templateUrl: './app.html',
  styleUrl: './app.scss'
})
export class App {
  protected readonly deviceName = signal('DDADAN DISPLAY #01');
  protected readonly playlist = signal<SignageItem[]>([
    {
      id: 'hero-image',
      type: 'image',
      title: '시그니처 메뉴 배너',
      src: 'https://images.unsplash.com/photo-1517248135467-4c7edcad34c4?auto=format&fit=crop&w=1600&q=80',
      duration: 8000
    },
    {
      id: 'promo-video',
      type: 'video',
      title: '프로모션 영상',
      src: 'https://interactive-examples.mdn.mozilla.net/media/cc0-videos/flower.mp4'
    },
    {
      id: 'event-image',
      type: 'image',
      title: '이벤트 공지',
      src: 'https://images.unsplash.com/photo-1552566626-52f8b828add9?auto=format&fit=crop&w=1600&q=80',
      duration: 8000
    }
  ]);

  protected readonly currentIndex = signal(0);
  protected readonly currentItem = computed(() => {
    const items = this.playlist();
    return items[this.currentIndex()] ?? null;
  });

  private imageTimer: ReturnType<typeof setTimeout> | null = null;

  constructor() {
    this.scheduleCurrentItem();
  }

  protected next(): void {
    const items = this.playlist();
    if (!items.length) {
      return;
    }

    this.currentIndex.update((index) => (index + 1) % items.length);
    this.scheduleCurrentItem();
  }

  protected previous(): void {
    const items = this.playlist();
    if (!items.length) {
      return;
    }

    this.currentIndex.update((index) => (index - 1 + items.length) % items.length);
    this.scheduleCurrentItem();
  }

  protected onVideoEnded(): void {
    this.next();
  }

  protected onImageLoaded(): void {
    this.scheduleCurrentItem();
  }

  private scheduleCurrentItem(): void {
    if (this.imageTimer) {
      clearTimeout(this.imageTimer);
      this.imageTimer = null;
    }

    const item = this.currentItem();
    if (!item || item.type !== 'image') {
      return;
    }

    this.imageTimer = setTimeout(() => this.next(), item.duration ?? 10000);
  }
}
