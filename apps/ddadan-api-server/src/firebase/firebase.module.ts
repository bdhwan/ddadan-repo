import { Global, Logger, Module } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { existsSync, readFileSync } from 'fs';
import * as admin from 'firebase-admin';
import { AppConfig } from '../config/configuration';

export const FIREBASE_ADMIN = Symbol('FIREBASE_ADMIN');

@Global()
@Module({
  providers: [
    {
      provide: FIREBASE_ADMIN,
      inject: [ConfigService],
      useFactory: (config: ConfigService<AppConfig, true>) => {
        const logger = new Logger('FirebaseAdmin');
        const path = config.get('firebase', { infer: true }).serviceAccountPath;

        if (admin.apps.length > 0) {
          return admin.app();
        }

        if (!existsSync(path)) {
          logger.warn(
            `Firebase service account not found at ${path}. ` +
              `Initializing without credentials — token verification will fail until you provide one.`,
          );
          return admin.initializeApp({ projectId: 'ddadan-dev' });
        }

        const serviceAccount = JSON.parse(
          readFileSync(path, 'utf8'),
        ) as admin.ServiceAccount;
        return admin.initializeApp({
          credential: admin.credential.cert(serviceAccount),
        });
      },
    },
  ],
  exports: [FIREBASE_ADMIN],
})
export class FirebaseModule {}
