import { provideHttpClient } from '@angular/common/http';
import { ApplicationConfig, provideBrowserGlobalErrorListeners } from '@angular/core';

// 이 앱은 라우팅을 쓰지 않는다(App 컴포넌트를 직접 부트스트랩). provideRouter를 두면
// 루트가 아닌 경로에서 서빙될 때 라우터가 매칭 실패로 에러를 던지므로 제거했다.
export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideHttpClient(),
  ],
};
