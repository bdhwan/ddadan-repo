import { Module } from '@nestjs/common';
import { ConfigModule, ConfigService } from '@nestjs/config';
import { ScheduleModule } from '@nestjs/schedule';
import { ServeStaticModule } from '@nestjs/serve-static';
import { AccountModule } from './account/account.module';
import { AssetsModule } from './assets/assets.module';
import { AuthModule } from './auth/auth.module';
import { loadConfig, AppConfig } from './config/configuration';
import { DatabaseModule } from './database/database.module';
import { DevicesModule } from './devices/devices.module';
import { FirebaseModule } from './firebase/firebase.module';
import { HeartbeatModule } from './heartbeat/heartbeat.module';
import { MonitorsModule } from './monitors/monitors.module';
import { PlayerModule } from './player/player.module';
import { PoliciesModule } from './policies/policies.module';
import { RedisModule } from './redis/redis.module';
import { ScreensModule } from './screens/screens.module';
import { StoresModule } from './stores/stores.module';
import { UsersModule } from './users/users.module';

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
    RedisModule,
    FirebaseModule,
    UsersModule,
    AuthModule,
    StoresModule,
    DevicesModule,
    MonitorsModule,
    AssetsModule,
    ScreensModule,
    PlayerModule,
    HeartbeatModule,
    PoliciesModule,
    AccountModule,
  ],
})
export class AppModule {}
