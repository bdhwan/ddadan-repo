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
}

export interface DeviceView {
  id: number;
  hardwareId: string;
  storeId: number | null;
  name: string | null;
  status: 'unregistered' | 'online' | 'offline';
  lastSeenAt: string | null;
  appVersion: string | null;
  monitors: MonitorView[];
  offlineAfterSeconds: number;
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

export interface ScreenLayoutItem {
  id: string;
  kind: 'image' | 'video' | 'text';
  assetId?: number | null;
  componentId?: number | null;
  text?: string;
  fontSize?: number;
  color?: string;
  background?: string;
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

export interface MeView {
  id: number;
  email: string | null;
  name: string | null;
  provider: string;
  firebaseUid: string;
  createdAt: string;
}

export interface PolicyDoc {
  id: number;
  kind: 'terms' | 'privacy';
  version: string;
  content: string;
  effectiveAt: string;
}

@Injectable({ providedIn: 'root' })
export class ApiService {
  private readonly http = inject(HttpClient);
  private readonly base = environment.apiBase;

  me(): Observable<MeView> {
    return this.http.get<MeView>(`${this.base}/users/me`);
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

  // Account
  withdraw(): Observable<void> {
    return this.http.delete<void>(`${this.base}/account`);
  }
}
