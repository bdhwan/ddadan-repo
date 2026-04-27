import {
  CanActivate,
  ExecutionContext,
  Inject,
  Injectable,
  Logger,
  UnauthorizedException,
} from '@nestjs/common';
import { Reflector } from '@nestjs/core';
import type { Request } from 'express';
import * as admin from 'firebase-admin';
import { FIREBASE_ADMIN } from '../firebase/firebase.module';
import { UsersService } from '../users/users.service';
import { PUBLIC_ROUTE_KEY } from './public.decorator';

export interface AuthContext {
  firebaseUid: string;
  email: string | null;
  provider: string;
  name: string | null;
  userId: number;
}

declare module 'express-serve-static-core' {
  interface Request {
    auth?: AuthContext;
  }
}

@Injectable()
export class FirebaseAuthGuard implements CanActivate {
  private readonly logger = new Logger(FirebaseAuthGuard.name);

  constructor(
    @Inject(FIREBASE_ADMIN) private readonly firebaseApp: admin.app.App,
    private readonly users: UsersService,
    private readonly reflector: Reflector,
  ) {}

  async canActivate(ctx: ExecutionContext): Promise<boolean> {
    const isPublic = this.reflector.getAllAndOverride<boolean>(
      PUBLIC_ROUTE_KEY,
      [ctx.getHandler(), ctx.getClass()],
    );
    if (isPublic) return true;

    const req = ctx.switchToHttp().getRequest<Request>();
    const header = req.headers.authorization ?? '';
    const [scheme, token] = header.split(' ');
    if (scheme !== 'Bearer' || !token) {
      throw new UnauthorizedException('Missing bearer token');
    }

    let decoded: admin.auth.DecodedIdToken;
    try {
      decoded = await this.firebaseApp.auth().verifyIdToken(token);
    } catch (err) {
      this.logger.debug(`token verification failed: ${(err as Error).message}`);
      throw new UnauthorizedException('Invalid token');
    }

    const user = await this.users.upsertFromFirebase({
      firebaseUid: decoded.uid,
      email: decoded.email ?? null,
      name: decoded.name ?? null,
      provider:
        decoded.firebase?.sign_in_provider ?? decoded.provider_id ?? 'unknown',
    });

    req.auth = {
      firebaseUid: decoded.uid,
      email: decoded.email ?? null,
      name: decoded.name ?? null,
      provider:
        decoded.firebase?.sign_in_provider ?? decoded.provider_id ?? 'unknown',
      userId: user.id,
    };
    return true;
  }
}
