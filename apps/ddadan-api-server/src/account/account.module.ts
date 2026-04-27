import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { Asset } from '../assets/asset.entity';
import { Device } from '../devices/device.entity';
import { Monitor } from '../monitors/monitor.entity';
import { ScreenComponent } from '../screens/screen-component.entity';
import { Screen } from '../screens/screen.entity';
import { Store } from '../stores/store.entity';
import { User } from '../users/user.entity';
import { AccountController } from './account.controller';
import { AccountService } from './account.service';

@Module({
  imports: [
    TypeOrmModule.forFeature([
      User,
      Store,
      Device,
      Monitor,
      Asset,
      Screen,
      ScreenComponent,
    ]),
  ],
  providers: [AccountService],
  controllers: [AccountController],
})
export class AccountModule {}
