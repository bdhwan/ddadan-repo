import { Controller, Get, ServiceUnavailableException } from '@nestjs/common';
import { DataSource } from 'typeorm';

@Controller('health')
export class HealthController {
  constructor(private readonly dataSource: DataSource) {}

  /** Liveness: 프로세스만 확인 (Docker/K8s livenessProbe) */
  @Get('live')
  live() {
    return { status: 'ok' };
  }

  /** Readiness: SQLite 연결 확인 (Docker/K8s readinessProbe) */
  @Get('ready')
  async ready() {
    try {
      await this.dataSource.query('SELECT 1');
    } catch {
      throw new ServiceUnavailableException({
        status: 'error',
        db: false,
      });
    }
    return { status: 'ok', db: true };
  }
}
