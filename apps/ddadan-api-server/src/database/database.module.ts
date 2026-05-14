import { Module } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { TypeOrmModule } from '@nestjs/typeorm';
import { AppConfig } from '../config/configuration';

@Module({
  imports: [
    TypeOrmModule.forRootAsync({
      inject: [ConfigService],
      useFactory: (config: ConfigService<AppConfig, true>) => {
        const db = config.get('db', { infer: true });
        return {
          type: 'better-sqlite3' as const,
          database: db.sqlitePath,
          synchronize: db.synchronize,
          logging: db.logging,
          autoLoadEntities: true,
        };
      },
    }),
  ],
})
export class DatabaseModule {}
