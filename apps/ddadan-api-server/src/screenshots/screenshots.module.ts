import { Module } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { MulterModule } from '@nestjs/platform-express';
import { TypeOrmModule } from '@nestjs/typeorm';
import { mkdirSync } from 'fs';
import { diskStorage } from 'multer';
import { join } from 'path';
import { AppConfig } from '../config/configuration';
import { AssetsService } from '../assets/assets.service';
import { Device } from '../devices/device.entity';
import { Screenshot } from './screenshot.entity';
import { ScreenshotsController } from './screenshots.controller';
import { ScreenshotsService } from './screenshots.service';

@Module({
  imports: [
    TypeOrmModule.forFeature([Screenshot, Device]),
    MulterModule.registerAsync({
      inject: [ConfigService],
      useFactory: (config: ConfigService<AppConfig, true>) => {
        const dir = join(config.get('assets', { infer: true }).dir, 'screenshots');
        mkdirSync(dir, { recursive: true });
        return {
          storage: diskStorage({
            destination: (_req, _file, cb) => cb(null, dir),
            filename: (_req, file, cb) =>
              cb(null, AssetsService.buildFilename(file.originalname || 'shot.jpg')),
          }),
        };
      },
    }),
  ],
  providers: [ScreenshotsService],
  controllers: [ScreenshotsController],
})
export class ScreenshotsModule {}
