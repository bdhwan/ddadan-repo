import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { StoresModule } from '../stores/stores.module';
import { UsersModule } from '../users/users.module';
import { ScreenComponent } from './screen-component.entity';
import { Screen } from './screen.entity';
import { ScreensController } from './screens.controller';
import { ScreensService } from './screens.service';

@Module({
  imports: [
    TypeOrmModule.forFeature([Screen, ScreenComponent]),
    StoresModule,
    UsersModule,
  ],
  providers: [ScreensService],
  controllers: [ScreensController],
  exports: [ScreensService],
})
export class ScreensModule {}
