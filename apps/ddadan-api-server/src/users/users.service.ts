import {
  Injectable,
  NotFoundException,
  OnModuleInit,
} from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { IsNull, Repository } from 'typeorm';
import { User } from './user.entity';

/** Single local user row (no Firebase); stable unique key in DB. */
export const LOCAL_USER_FIREBASE_UID = 'local';

@Injectable()
export class UsersService implements OnModuleInit {
  private localUserId!: number;

  constructor(
    @InjectRepository(User)
    private readonly users: Repository<User>,
  ) {}

  async onModuleInit() {
    let u = await this.users.findOne({
      where: { firebaseUid: LOCAL_USER_FIREBASE_UID, deletedAt: IsNull() },
    });
    if (!u) {
      u = this.users.create({
        firebaseUid: LOCAL_USER_FIREBASE_UID,
        email: 'local@localhost',
        name: 'Local user',
        provider: 'local',
      });
      u = await this.users.save(u);
    }
    this.localUserId = u.id;
  }

  getLocalUserId(): number {
    return this.localUserId;
  }

  async findById(id: number): Promise<User> {
    const user = await this.users.findOne({
      where: { id, deletedAt: IsNull() },
    });
    if (!user) throw new NotFoundException('User not found');
    return user;
  }

  async softDelete(id: number): Promise<void> {
    await this.users.softDelete(id);
  }
}
