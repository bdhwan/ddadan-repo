import { SlicePipe } from '@angular/common';
import { Component, inject, OnInit, signal } from '@angular/core';
import { ActivatedRoute } from '@angular/router';
import { ApiService, PolicyDoc } from '../api.service';

@Component({
  standalone: true,
  imports: [SlicePipe],
  template: `
    <div class="panel" style="max-width:760px">
      <h2>{{ title() }}</h2>
      @if (doc()) {
        <p class="muted">v{{ doc()!.version }} · 시행 {{ doc()!.effectiveAt | slice: 0:10 }}</p>
        <pre style="white-space:pre-wrap; font-family:inherit">{{ doc()!.content }}</pre>
      } @else {
        <p class="muted">불러오는 중...</p>
      }
    </div>
  `,
})
export class PolicyPage implements OnInit {
  private readonly api = inject(ApiService);
  private readonly route = inject(ActivatedRoute);
  readonly doc = signal<PolicyDoc | null>(null);
  readonly title = signal('정책');

  ngOnInit() {
    const kind = this.route.snapshot.data['kind'] as 'terms' | 'privacy';
    this.title.set(kind === 'terms' ? '이용약관' : '개인정보처리방침');
    this.api.currentPolicies().subscribe((r) => {
      this.doc.set(kind === 'terms' ? r.terms : r.privacy);
    });
  }
}
