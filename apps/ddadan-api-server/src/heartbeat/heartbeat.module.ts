import { Module } from '@nestjs/common';
import { DevicesModule } from '../devices/devices.module';
import { HeartbeatService } from './heartbeat.service';

@Module({
  imports: [DevicesModule],
  providers: [HeartbeatService],
})
export class HeartbeatModule {}
