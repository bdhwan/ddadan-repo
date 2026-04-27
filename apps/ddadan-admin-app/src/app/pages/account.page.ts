import { SlicePipe } from '@angular/common';
import { Component, inject, OnInit, signal } from '@angular/core';
import { Router, RouterLink } from '@angular/router';
import { ApiService, MeView } from '../api.service';
import { AuthService } from '../auth.service';

@Component({
  standalone: true,
  imports: [RouterLink, SlicePipe],
  template: `
    <h1>계정</h1>
    <div class="panel" style="max-width:560px">
      @if (me(); as u) {
        <p><strong>{{ u.email ?? '(이메일 없음)' }}</strong></p>
        <p class="muted">로그인 방식: {{ u.provider }}</p>
        <p class="muted">가입일: {{ u.createdAt | slice: 0:10 }}</p>
      }
      <hr style="margin:16px 0; border:none; border-top:1px solid var(--border)" />
      <p>
        <a routerLink="/terms">이용약관</a> · <a routerLink="/privacy">개인정보처리방침</a>
      </p>
      <hr style="margin:16px 0; border:none; border-top:1px solid var(--border)" />
      <h3 style="color:var(--danger)">회원 탈퇴</h3>
      <p class="muted">
        탈퇴 시 모든 매장, 디바이스, 에셋, 화면 데이터가 소프트 삭제됩니다.
        동일 이메일로 즉시 재가입하실 수 있습니다.
      </p>
      <button class="danger" (click)="withdraw()" [disabled]="busy()">회원 탈퇴</button>
    </div>
  `,
})
export class AccountPage implements OnInit {
  private readonly api = inject(ApiService);
  private readonly auth = inject(AuthService);
  private readonly router = inject(Router);
  readonly me = signal<MeView | null>(null);
  readonly busy = signal(false);

  ngOnInit() {
    this.api.me().subscribe((u) => this.me.set(u));
  }

  withdraw() {
    if (!confirm('정말로 탈퇴하시겠어요? 모든 데이터가 삭제됩니다.')) return;
    this.busy.set(true);
    this.api.withdraw().subscribe({
      next: async () => {
        await this.auth.signOut();
        this.router.navigateByUrl('/login');
      },
      error: () => this.busy.set(false),
    });
  }
}
