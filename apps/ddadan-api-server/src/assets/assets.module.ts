import { Module } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { MulterModule } from '@nestjs/platform-express';
import { TypeOrmModule } from '@nestjs/typeorm';
import { mkdirSync } from 'fs';
import { diskStorage } from 'multer';
import { AppConfig } from '../config/configuration';
import { StoresModule } from '../stores/stores.module';
import { Asset } from './asset.entity';
import { AssetsController } from './assets.controller';
import { AssetsService } from './assets.service';

@Module({
  imports: [
    TypeOrmModule.forFeature([Asset]),
    MulterModule.registerAsync({
      inject: [ConfigService],
      useFactory: (config: ConfigService<AppConfig, true>) => {
        const dir = config.get('assets', { infer: true }).dir;
        mkdirSync(dir, { recursive: true });
        return {
          storage: diskStorage({
            destination: (_req, _file, cb) => cb(null, dir),
            filename: (_req, file, cb) =>
              cb(null, AssetsService.buildFilename(file.originalname)),
          }),
        };
      },
    }),
    StoresModule,
  ],
  providers: [AssetsService],
  controllers: [AssetsController],
  exports: [AssetsService],
})
export class AssetsModule {}
