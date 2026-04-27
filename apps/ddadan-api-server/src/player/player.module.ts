import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { Asset } from '../assets/asset.entity';
import { AssetsModule } from '../assets/assets.module';
import { Device } from '../devices/device.entity';
import { Monitor } from '../monitors/monitor.entity';
import { Screen } from '../screens/screen.entity';
import { PlayerController } from './player.controller';

@Module({
  imports: [
    TypeOrmModule.forFeature([Device, Monitor, Screen, Asset]),
    AssetsModule,
  ],
  controllers: [PlayerController],
})
export class PlayerModule {}
