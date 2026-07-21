import { Module } from '@nestjs/common';
import { ConfigModule, ConfigService } from '@nestjs/config';
import { ScheduleModule } from '@nestjs/schedule';
import { ServeStaticModule } from '@nestjs/serve-static';
import { AccountModule } from './account/account.module';
import { ApksModule } from './apks/apks.module';
import { AssetsModule } from './assets/assets.module';
import { loadConfig, AppConfig } from './config/configuration';
import { DatabaseModule } from './database/database.module';
import { DevicesModule } from './devices/devices.module';
import { HeartbeatModule } from './heartbeat/heartbeat.module';
import { MonitorsModule } from './monitors/monitors.module';
import { PlayerModule } from './player/player.module';
import { PoliciesModule } from './policies/policies.module';
import { ScreensModule } from './screens/screens.module';
import { ScreenshotsModule } from './screenshots/screenshots.module';
import { StoresModule } from './stores/stores.module';
import { UsersModule } from './users/users.module';
import { HealthController } from './health/health.controller';

@Module({
  imports: [
    ConfigModule.forRoot({
      isGlobal: true,
      load: [loadConfig],
    }),
    ScheduleModule.forRoot(),
    ServeStaticModule.forRootAsync({
      inject: [ConfigService],
      useFactory: (config: ConfigService<AppConfig, true>) => {
        const a = config.get('assets', { infer: true });
        return [{ rootPath: a.dir, serveRoot: a.publicPath }];
      },
    }),
    DatabaseModule,
    UsersModule,
    StoresModule,
    DevicesModule,
    MonitorsModule,
    AssetsModule,
    ScreensModule,
    ScreenshotsModule,
    ApksModule,
    PlayerModule,
    HeartbeatModule,
    PoliciesModule,
    AccountModule,
  ],
  controllers: [HealthController],
})
export class AppModule {}
