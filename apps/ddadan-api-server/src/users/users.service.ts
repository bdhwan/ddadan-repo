import { Injectable, NotFoundException } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { IsNull, Repository } from 'typeorm';
import { User } from './user.entity';

export interface UpsertFromFirebaseInput {
  firebaseUid: string;
  email: string | null;
  name: string | null;
  provider: string;
}

@Injectable()
export class UsersService {
  constructor(
    @InjectRepository(User)
    private readonly users: Repository<User>,
  ) {}

  async upsertFromFirebase(input: UpsertFromFirebaseInput): Promise<User> {
    const existing = await this.users.findOne({
      where: { firebaseUid: input.firebaseUid, deletedAt: IsNull() },
    });
    if (existing) {
      let dirty = false;
      if (existing.email !== input.email) {
        existing.email = input.email;
        dirty = true;
      }
      if (input.name && existing.name !== input.name) {
        existing.name = input.name;
        dirty = true;
      }
      if (existing.provider !== input.provider) {
        existing.provider = input.provider;
        dirty = true;
      }
      return dirty ? this.users.save(existing) : existing;
    }
    const created = this.users.create({
      firebaseUid: input.firebaseUid,
      email: input.email,
      name: input.name,
      provider: input.provider,
    });
    return this.users.save(created);
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
