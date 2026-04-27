import { ForbiddenException, Injectable, NotFoundException } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { IsNull, Repository } from 'typeorm';
import { CreateStoreDto, UpdateStoreDto } from './dto/store.dto';
import { Store } from './store.entity';

@Injectable()
export class StoresService {
  constructor(
    @InjectRepository(Store)
    private readonly stores: Repository<Store>,
  ) {}

  list(ownerUserId: number): Promise<Store[]> {
    return this.stores.find({
      where: { ownerUserId, deletedAt: IsNull() },
      order: { id: 'ASC' },
    });
  }

  async getOwned(id: number, ownerUserId: number): Promise<Store> {
    const store = await this.stores.findOne({
      where: { id, deletedAt: IsNull() },
    });
    if (!store) throw new NotFoundException('Store not found');
    if (store.ownerUserId !== ownerUserId) {
      throw new ForbiddenException('Not your store');
    }
    return store;
  }

  create(ownerUserId: number, dto: CreateStoreDto): Promise<Store> {
    const created = this.stores.create({
      ownerUserId,
      name: dto.name,
      businessType: dto.businessType ?? null,
      timezone: dto.timezone ?? 'Asia/Seoul',
    });
    return this.stores.save(created);
  }

  async update(
    id: number,
    ownerUserId: number,
    dto: UpdateStoreDto,
  ): Promise<Store> {
    const store = await this.getOwned(id, ownerUserId);
    if (dto.name !== undefined) store.name = dto.name;
    if (dto.businessType !== undefined) store.businessType = dto.businessType;
    if (dto.timezone !== undefined) store.timezone = dto.timezone;
    return this.stores.save(store);
  }

  async remove(id: number, ownerUserId: number): Promise<void> {
    await this.getOwned(id, ownerUserId);
    await this.stores.softDelete(id);
  }
}
