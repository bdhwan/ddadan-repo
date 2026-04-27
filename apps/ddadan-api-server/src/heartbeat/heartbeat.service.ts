import { Inject, Injectable, Logger } from '@nestjs/common';
import { Cron, CronExpression } from '@nestjs/schedule';
import type Redis from 'ioredis';
import { DevicesService } from '../devices/devices.service';
import { REDIS_CLIENT } from '../redis/redis.module';

@Injectable()
export class HeartbeatService {
  private readonly logger = new Logger(HeartbeatService.name);

  constructor(
    private readonly devices: DevicesService,
    @Inject(REDIS_CLIENT) private readonly redis: Redis,
  ) {}

  @Cron(CronExpression.EVERY_30_SECONDS)
  async sweepOfflineDevices() {
    const all = await this.devices.listAllRegistered();
    for (const device of all) {
      const alive = await this.redis.exists(`device:${device.id}:heartbeat`);
      if (!alive && device.status === 'online') {
        this.logger.log(`Marking device ${device.id} offline (heartbeat expired)`);
        await this.devices.markOfflineByExpiry(device.id);
      }
    }
  }
}
