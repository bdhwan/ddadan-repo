import { spawn, type ChildProcess } from 'child_process';

export interface KioskOptions {
  url: string;
  browserBin?: string | undefined;
  enabled: boolean;
}

const DEFAULT_BINS = ['chromium-browser', 'chromium', 'google-chrome'];

// Hardened chromium flags shared by the direct-launch fallback path.
// (When a launcher script is configured, the script owns the flag set.)
const CHROMIUM_FLAGS = [
  '--kiosk',
  '--start-fullscreen',
  '--noerrdialogs',
  '--disable-translate',
  '--disable-infobars',
  '--no-first-run',
  '--fast',
  '--fast-start',
  // Prevent the "Chrome didn't shut down correctly" / session restore bubble
  // that otherwise blocks the page on every unclean exit (power loss, kill).
  '--disable-session-crashed-bubble',
  '--disable-features=Translate',
  '--disable-background-networking',
  '--check-for-update-interval=31536000',
  '--overscroll-history-navigation=0',
  '--disable-pinch',
];

export class KioskSupervisor {
  private current: ChildProcess | null = null;
  private targetUrl: string | null = null;
  private restarting = false;

  constructor(
    private readonly options: {
      browserBin?: string | undefined;
      launcher?: string | undefined;
      enabled: boolean;
    },
  ) {}

  setUrl(url: string) {
    if (url === this.targetUrl) return;
    this.targetUrl = url;
    this.respawn();
  }

  hasUrl(): boolean {
    return this.targetUrl !== null;
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

    const { cmd, args } = this.buildCommand(this.targetUrl);
    console.log(`[kiosk] launching: ${cmd} ${args.join(' ')}`);
    // Pass through the full environment so DISPLAY / XAUTHORITY / WAYLAND_DISPLAY /
    // XDG_RUNTIME_DIR injected by the systemd unit reach chromium. Without this,
    // chromium can't find the display and exits immediately (blank monitor).
    const child = spawn(cmd, args, {
      stdio: 'ignore',
      detached: false,
      env: { ...process.env },
    });
    this.current = child;
    child.on('exit', (code) => {
      console.log(`[kiosk] browser exited code=${code}`);
      this.current = null;
      if (!this.restarting) {
        setTimeout(() => this.respawn(), 3000);
      }
    });
    child.on('error', (err) => {
      console.warn(`[kiosk] spawn error: ${err.message}`);
      this.current = null;
      if (!this.restarting) {
        setTimeout(() => this.respawn(), 3000);
      }
    });
  }

  private buildCommand(url: string): { cmd: string; args: string[] } {
    // When a launcher script is configured it owns display detection, screen
    // blanking, hotplug recovery and the chromium flag set — we just hand it the URL.
    if (this.options.launcher) {
      return { cmd: this.options.launcher, args: [url] };
    }
    const bin = this.options.browserBin ?? DEFAULT_BINS[0]!;
    return { cmd: bin, args: [...CHROMIUM_FLAGS, url] };
  }
}
