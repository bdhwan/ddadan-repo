import { Injectable, Logger } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { Cron, CronExpression } from '@nestjs/schedule';
import { AppConfig } from '../config/configuration';
import { DevicesService } from '../devices/devices.service';

@Injectable()
export class HeartbeatService {
  private readonly logger = new Logger(HeartbeatService.name);

  constructor(
    private readonly devices: DevicesService,
    private readonly config: ConfigService<AppConfig, true>,
  ) {}

  @Cron(CronExpression.EVERY_30_SECONDS)
  async sweepOfflineDevices() {
    const ttl = this.config.get('heartbeat', { infer: true }).offlineAfterSeconds;
    const all = await this.devices.listAllRegistered();
    for (const device of all) {
      if (device.status !== 'online') continue;
      const live = DevicesService.liveStatusFromLastSeen(
        device.lastSeenAt,
        ttl,
        device.status,
      );
      if (live === 'offline') {
        this.logger.log(`Marking device ${device.id} offline (heartbeat expired)`);
        await this.devices.markOfflineByExpiry(device.id);
      }
    }
  }
}
