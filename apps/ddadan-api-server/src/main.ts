import { mkdirSync } from 'fs';
import { dirname } from 'path';
import { ValidationPipe } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { NestFactory } from '@nestjs/core';
import { AppModule } from './app.module';
import { AppConfig } from './config/configuration';

async function bootstrap() {
  const app = await NestFactory.create(AppModule);
  const config = app.get(ConfigService<AppConfig, true>);
  const dbPath = config.get('db', { infer: true }).sqlitePath;
  const assetsDir = config.get('assets', { infer: true }).dir;
  mkdirSync(dirname(dbPath), { recursive: true });
  mkdirSync(assetsDir, { recursive: true });

  app.setGlobalPrefix('api');
  app.useGlobalPipes(
    new ValidationPipe({
      whitelist: true,
      forbidNonWhitelisted: false,
      transform: true,
      transformOptions: { enableImplicitConversion: true },
    }),
  );
  app.enableCors({
    origin: true,
    credentials: true,
  });

  const host = config.get('host', { infer: true });
  const port = config.get('port', { infer: true });
  await app.listen(port, host);
}

bootstrap();
