import { Module } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { MulterModule } from '@nestjs/platform-express';
import { TypeOrmModule } from '@nestjs/typeorm';
import { mkdirSync } from 'fs';
import { diskStorage } from 'multer';
import { join } from 'path';
import { AppConfig } from '../config/configuration';
import { AssetsService } from '../assets/assets.service';
import { Apk } from './apk.entity';
import { ApksController } from './apks.controller';
import { ApksService } from './apks.service';

@Module({
  imports: [
    TypeOrmModule.forFeature([Apk]),
    MulterModule.registerAsync({
      inject: [ConfigService],
      useFactory: (config: ConfigService<AppConfig, true>) => {
        const dir = join(config.get('assets', { infer: true }).dir, 'apks');
        mkdirSync(dir, { recursive: true });
        return {
          storage: diskStorage({
            destination: (_req, _file, cb) => cb(null, dir),
            filename: (_req, file, cb) =>
              cb(null, AssetsService.buildFilename(file.originalname || 'app.apk')),
          }),
        };
      },
    }),
  ],
  providers: [ApksService],
  controllers: [ApksController],
})
export class ApksModule {}
