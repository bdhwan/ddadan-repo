import axios, { type AxiosInstance } from 'axios';
import type { PiConfig } from './config';
import type { MonitorReport } from './monitors';

export interface CheckResponse {
  registered: boolean;
  device?: {
    id: number;
    name: string | null;
    hardwareId: string;
  };
}

export class DdadanApi {
  private readonly http: AxiosInstance;

  constructor(config: PiConfig) {
    this.http = axios.create({
      baseURL: config.apiBase,
      timeout: 8000,
    });
  }

  async check(hardwareId: string, monitors: MonitorReport[]): Promise<CheckResponse> {
    const res = await this.http.post<CheckResponse>('/devices/check', {
      hardwareId,
      monitors,
    });
    return res.data;
  }

  async heartbeat(
    hardwareId: string,
    appVersion: string,
    monitors: MonitorReport[],
  ): Promise<void> {
    await this.http.post('/devices/heartbeat', {
      hardwareId,
      appVersion,
      monitors,
    });
  }
}
