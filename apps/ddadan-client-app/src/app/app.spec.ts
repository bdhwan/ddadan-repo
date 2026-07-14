import { TestBed } from '@angular/core/testing';
import { provideHttpClient } from '@angular/common/http';
import { App } from './app';

describe('App', () => {
  beforeEach(async () => {
    await TestBed.configureTestingModule({
      imports: [App],
      providers: [provideHttpClient()],
    }).compileComponents();
  });

  it('should create the app', () => {
    const fixture = TestBed.createComponent(App);
    const app = fixture.componentInstance;
    expect(app).toBeTruthy();
  });

  it('formats viewport-relative and legacy pixel font sizes', () => {
    const fixture = TestBed.createComponent(App);
    const app = fixture.componentInstance as any;

    expect(app.fontCss({ fontSize: 3.7037037037, fontUnit: 'vh' })).toBe('3.7037037037vh');
    expect(app.fontCss({ fontSize: 40 })).toBe('40px');
    expect(app.fontCss({})).toBeNull();
  });
});
