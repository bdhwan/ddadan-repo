import { HttpClient } from '@angular/common/http';
import { Injectable, inject } from '@angular/core';
import { Observable } from 'rxjs';
import { environment } from '../environment';

export interface Store {
  id: number;
  name: string;
  businessType: string | null;
  timezone: string;
  ownerUserId: number;
  createdAt: string;
}

export interface MonitorView {
  id: number;
  deviceId: number;
  slot: number;
  resolutionW: number;
  resolutionH: number;
  positionX: number;
  positionY: number;
  currentScreenId: number | null;
  rotationScreenIds?: number[];
  rotationIntervalMs?: number;
  rotationFadeMs?: number;
}

export interface ApkInfo {
  versionCode: number;
  versionName: string | null;
  applicationId: string | null;
  url?: string | null;
  sizeBytes?: number | null;
  createdAt?: string | null;
}

export interface DeviceView {
  id: number;
  hardwareId: string;
  storeId: number | null;
  storeName?: string | null;
  name: string | null;
  status: 'unregistered' | 'online' | 'offline';
  lastSeenAt: string | null;
  appVersion: string | null;
  watchdogVersion?: string | null;
  cpuPercent?: number | null;
  ramUsedMb?: number | null;
  ramTotalMb?: number | null;
  diskUsedPercent?: number | null;
  monitors: MonitorView[];
  offlineAfterSeconds: number;
}

export interface CommandView {
  id: number;
  type: 'reboot' | 'screenOn' | 'screenOff' | 'updateApp' | 'shell' | 'screenshot';
  payload: string | null;
  status: 'pending' | 'done' | 'failed';
  result: string | null;
  createdAt: string;
  ackedAt: string | null;
}

export interface AssetView {
  id: number;
  type: 'image' | 'video' | 'text';
  originalName: string;
  mimeType: string | null;
  sizeBytes: number;
  url: string | null;
  textContent: string | null;
  storeId: number | null;
  createdAt: string;
}

export interface ScreenshotView {
  id: number;
  url: string;
  sizeBytes: number;
  createdAt: string;
}

export interface ScreenLayoutItem {
  id: string;
  kind: 'image' | 'video' | 'text';
  assetId?: number | null;
  componentId?: number | null;
  text?: string;
  fontSize?: number;
  fontUnit?: 'px' | 'vh';
  color?: string;
  background?: string;
  opacity?: number;
  fontWeight?: number;
  textAlign?: 'left' | 'center' | 'right';
  lineHeight?: number;
  textVariant?: 'plain' | 'menuLine';
  textSecondary?: string;
  x: number;
  y: number;
  width: number;
  height: number;
  zIndex?: number;
  durationMs?: number;
}

export interface ScreenView {
  id: number;
  ownerUserId: number;
  storeId: number | null;
  name: string;
  width: number;
  height: number;
  layout: { background?: string; items: ScreenLayoutItem[] };
}

export interface PolicyDoc {
  id: number;
  kind: 'terms' | 'privacy';
  version: string;
  content: string;
  effectiveAt: string;
}

export interface PlayerScreenItem {
  id: string;
  kind: 'image' | 'video' | 'text';
  url?: string;
  text?: string;
  fontSize?: number;
  fontUnit?: 'px' | 'vh';
  color?: string;
  background?: string;
  opacity?: number;
  fontWeight?: number;
  textAlign?: 'left' | 'center' | 'right';
  lineHeight?: number;
  textVariant?: 'plain' | 'menuLine';
  textSecondary?: string;
  x: number;
  y: number;
  width: number;
  height: number;
  zIndex?: number;
}

export interface PlayerSlidePayload {
  width: number;
  height: number;
  background?: string;
  items: PlayerScreenItem[];
}

export interface PlayerScreenResponse {
  registered: boolean;
  deviceName: string | null;
  mode?: 'single' | 'rotation';
  width: number;
  height: number;
  background?: string;
  items: PlayerScreenItem[];
  rotation?: {
    intervalMs: number;
    fadeMs: number;
    slides: PlayerSlidePayload[];
  };
  isFallback?: boolean;
}

@Injectable({ providedIn: 'root' })
export class ApiService {
  private readonly http = inject(HttpClient);
  private readonly base = environment.apiBase;

  /**
   * 정적 파일 기준 URL. dev(apiBase=`http://host:port/api`)에서는 API 호스트,
   * 배포(apiBase=`/api`)에서는 빈 문자열 → 브라우저가 현재 호스트의 `/static/...` 사용.
   */
  readonly assetOrigin = this.base.replace(/\/api\/?$/, '');

  /** API가 돌려주는 /static/... 상대 경로를 img·video src용 절대 URL로 */
  absoluteAssetUrl(path: string | null | undefined): string {
    if (path == null || path === '') return '';
    const p = path.trim();
    if (p.startsWith('http://') || p.startsWith('https://')) return p;
    const prefix = p.startsWith('/') ? p : `/${p}`;
    return `${this.assetOrigin}${prefix}`;
  }

  // Policies
  currentPolicies(): Observable<{ terms: PolicyDoc | null; privacy: PolicyDoc | null }> {
    return this.http.get<{ terms: PolicyDoc | null; privacy: PolicyDoc | null }>(
      `${this.base}/policies/current`,
    );
  }
  acceptPolicies(documentIds: number[]): Observable<{ ok: boolean }> {
    return this.http.post<{ ok: boolean }>(`${this.base}/policies/accept`, {
      documentIds,
    });
  }

  // Stores
  listStores(): Observable<Store[]> {
    return this.http.get<Store[]>(`${this.base}/stores`);
  }
  createStore(name: string): Observable<Store> {
    return this.http.post<Store>(`${this.base}/stores`, { name });
  }
  deleteStore(id: number): Observable<void> {
    return this.http.delete<void>(`${this.base}/stores/${id}`);
  }

  // Devices
  listAllDevices(): Observable<DeviceView[]> {
    return this.http.get<DeviceView[]>(`${this.base}/devices`);
  }
  /** 서버에 올라온 최신 APK(플레이어 OTA 페이로드). */
  latestApk(applicationId: string): Observable<ApkInfo> {
    return this.http.get<ApkInfo>(`${this.base}/apks/latest`, {
      params: { applicationId },
    });
  }
  listDevices(storeId: number): Observable<DeviceView[]> {
    return this.http.get<DeviceView[]>(`${this.base}/stores/${storeId}/devices`);
  }
  registerDevice(storeId: number, hardwareId: string, name?: string): Observable<unknown> {
    return this.http.post(`${this.base}/devices`, { storeId, hardwareId, name });
  }
  getDevice(id: number): Observable<DeviceView> {
    return this.http.get<DeviceView>(`${this.base}/devices/${id}`);
  }
  unregisterDevice(id: number): Observable<void> {
    return this.http.delete<void>(`${this.base}/devices/${id}`);
  }
  listDeviceScreenshots(id: number, limit = 10): Observable<ScreenshotView[]> {
    return this.http.get<ScreenshotView[]>(
      `${this.base}/devices/${id}/screenshots?limit=${limit}`,
    );
  }
  sendCommand(
    id: number,
    type: CommandView['type'],
    payload?: string,
  ): Observable<CommandView> {
    return this.http.post<CommandView>(`${this.base}/devices/${id}/commands`, {
      type,
      payload,
    });
  }
  listCommands(id: number, limit = 20): Observable<CommandView[]> {
    return this.http.get<CommandView[]>(
      `${this.base}/devices/${id}/commands?limit=${limit}`,
    );
  }

  // Monitors
  updateMonitorPosition(id: number, positionX: number, positionY: number) {
    return this.http.patch<MonitorView>(`${this.base}/monitors/${id}/position`, {
      positionX,
      positionY,
    });
  }
  assignScreen(monitorId: number, screenId: number | null) {
    return this.http.patch<MonitorView>(`${this.base}/monitors/${monitorId}/screen`, {
      screenId,
    });
  }
  setMonitorRotation(
    monitorId: number,
    body: { screenIds: number[]; intervalMs: number; fadeMs: number },
  ): Observable<MonitorView> {
    return this.http.patch<MonitorView>(`${this.base}/monitors/${monitorId}/rotation`, body);
  }

  // Assets
  listAssets(): Observable<AssetView[]> {
    return this.http.get<AssetView[]>(`${this.base}/assets`);
  }
  uploadAsset(file: File, storeId?: number): Observable<AssetView> {
    const form = new FormData();
    form.append('file', file);
    if (storeId !== undefined) form.append('storeId', String(storeId));
    return this.http.post<AssetView>(`${this.base}/assets/upload`, form);
  }
  createTextAsset(originalName: string, textContent: string): Observable<AssetView> {
    return this.http.post<AssetView>(`${this.base}/assets/text`, {
      originalName,
      textContent,
    });
  }
  deleteAsset(id: number): Observable<void> {
    return this.http.delete<void>(`${this.base}/assets/${id}`);
  }

  // Screens
  listScreens(storeId?: number): Observable<ScreenView[]> {
    const q = storeId ? `?storeId=${storeId}` : '';
    return this.http.get<ScreenView[]>(`${this.base}/screens${q}`);
  }
  getScreen(id: number): Observable<ScreenView> {
    return this.http.get<ScreenView>(`${this.base}/screens/${id}`);
  }
  createScreen(payload: Partial<ScreenView>): Observable<ScreenView> {
    return this.http.post<ScreenView>(`${this.base}/screens`, payload);
  }
  updateScreen(id: number, payload: Partial<ScreenView>): Observable<ScreenView> {
    return this.http.patch<ScreenView>(`${this.base}/screens/${id}`, payload);
  }
  deleteScreen(id: number): Observable<void> {
    return this.http.delete<void>(`${this.base}/screens/${id}`);
  }

  saveComponent(name: string, item: ScreenLayoutItem) {
    return this.http.post(`${this.base}/screen-components`, {
      name,
      kind: item.kind,
      payload: item,
    });
  }
  listComponents() {
    return this.http.get<Array<{ id: number; name: string; kind: string; payload: ScreenLayoutItem | ScreenLayoutItem[] }>>(
      `${this.base}/screen-components`,
    );
  }

  getPlayerScreen(hardwareId: string, slot = 0): Observable<PlayerScreenResponse> {
    return this.http.get<PlayerScreenResponse>(
      `${this.base}/player/${encodeURIComponent(hardwareId)}/screen?slot=${slot}`,
    );
  }
}
