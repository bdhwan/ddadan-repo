import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { Monitor } from '../monitors/monitor.entity';
import { StoresModule } from '../stores/stores.module';
import { UsersModule } from '../users/users.module';
import { Device } from './device.entity';
import { DevicesController } from './devices.controller';
import { DevicesService } from './devices.service';

@Module({
  imports: [TypeOrmModule.forFeature([Device, Monitor]), StoresModule, UsersModule],
  providers: [DevicesService],
  controllers: [DevicesController],
  exports: [DevicesService],
})
export class DevicesModule {}
