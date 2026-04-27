import { spawn, type ChildProcess } from 'child_process';

export interface KioskOptions {
  url: string;
  browserBin?: string | undefined;
  enabled: boolean;
}

const DEFAULT_BINS = ['chromium-browser', 'chromium', 'google-chrome'];

export class KioskSupervisor {
  private current: ChildProcess | null = null;
  private targetUrl: string | null = null;
  private restarting = false;

  constructor(private readonly options: { browserBin?: string | undefined; enabled: boolean }) {}

  setUrl(url: string) {
    if (url === this.targetUrl) return;
    this.targetUrl = url;
    this.respawn();
  }

  shutdown() {
    if (this.current && !this.current.killed) {
      this.current.kill('SIGTERM');
    }
    this.current = null;
  }

  private respawn() {
    if (this.current && !this.current.killed) {
      this.current.kill('SIGTERM');
    }
    if (!this.options.enabled || !this.targetUrl) {
      console.log(`[kiosk] (dry-run) would launch: ${this.targetUrl ?? '<none>'}`);
      return;
    }
    const bin = this.options.browserBin ?? DEFAULT_BINS[0]!;
    const args = [
      '--kiosk',
      '--noerrdialogs',
      '--disable-translate',
      '--no-first-run',
      '--fast',
      '--fast-start',
      '--disable-infobars',
      this.targetUrl,
    ];
    console.log(`[kiosk] launching: ${bin} ${this.targetUrl}`);
    const child = spawn(bin, args, { stdio: 'ignore', detached: false });
    this.current = child;
    child.on('exit', (code) => {
      console.log(`[kiosk] browser exited code=${code}`);
      this.current = null;
      if (!this.restarting) {
        setTimeout(() => this.respawn(), 3000);
      }
    });
  }
}
