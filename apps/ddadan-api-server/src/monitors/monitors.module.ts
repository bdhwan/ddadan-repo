import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { Device } from '../devices/device.entity';
import { Screen } from '../screens/screen.entity';
import { StoresModule } from '../stores/stores.module';
import { UsersModule } from '../users/users.module';
import { Monitor } from './monitor.entity';
import { MonitorsController } from './monitors.controller';
import { MonitorsService } from './monitors.service';

@Module({
  imports: [
    TypeOrmModule.forFeature([Monitor, Device, Screen]),
    StoresModule,
    UsersModule,
  ],
  providers: [MonitorsService],
  controllers: [MonitorsController],
  exports: [MonitorsService],
})
export class MonitorsModule {}
