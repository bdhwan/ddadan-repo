import { ForbiddenException, Injectable, NotFoundException } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { IsNull, Repository } from 'typeorm';
import { StoresService } from '../stores/stores.service';
import {
  CreateScreenComponentDto,
  CreateScreenDto,
  UpdateScreenDto,
} from './dto/screen.dto';
import { ScreenComponent } from './screen-component.entity';
import { Screen, ScreenLayoutItem } from './screen.entity';

@Injectable()
export class ScreensService {
  constructor(
    @InjectRepository(Screen) private readonly screens: Repository<Screen>,
    @InjectRepository(ScreenComponent)
    private readonly components: Repository<ScreenComponent>,
    private readonly stores: StoresService,
  ) {}

  list(ownerUserId: number, storeId?: number) {
    const where: Record<string, unknown> = { ownerUserId, deletedAt: IsNull() };
    if (storeId) where.storeId = storeId;
    return this.screens.find({ where, order: { id: 'DESC' } });
  }

  async findOwned(id: number, ownerUserId: number): Promise<Screen> {
    const screen = await this.screens.findOne({
      where: { id, deletedAt: IsNull() },
    });
    if (!screen) throw new NotFoundException('Screen not found');
    if (screen.ownerUserId !== ownerUserId) {
      throw new ForbiddenException('Not your screen');
    }
    return screen;
  }

  async create(ownerUserId: number, dto: CreateScreenDto): Promise<Screen> {
    if (dto.storeId) {
      await this.stores.getOwned(dto.storeId, ownerUserId);
    }
    const screen = this.screens.create({
      ownerUserId,
      storeId: dto.storeId ?? null,
      name: dto.name,
      width: dto.width,
      height: dto.height,
      layout: dto.layout,
    });
    return this.screens.save(screen);
  }

  async update(
    id: number,
    ownerUserId: number,
    dto: UpdateScreenDto,
  ): Promise<Screen> {
    const screen = await this.findOwned(id, ownerUserId);
    if (dto.name !== undefined) screen.name = dto.name;
    if (dto.width !== undefined) screen.width = dto.width;
    if (dto.height !== undefined) screen.height = dto.height;
    if (dto.layout !== undefined) screen.layout = dto.layout;
    return this.screens.save(screen);
  }

  async remove(id: number, ownerUserId: number): Promise<void> {
    await this.findOwned(id, ownerUserId);
    await this.screens.softDelete(id);
  }

  listComponents(ownerUserId: number) {
    return this.components.find({
      where: { ownerUserId, deletedAt: IsNull() },
      order: { id: 'DESC' },
    });
  }

  async createComponent(
    ownerUserId: number,
    dto: CreateScreenComponentDto,
  ): Promise<ScreenComponent> {
    const c = this.components.create({
      ownerUserId,
      name: dto.name,
      kind: dto.kind,
      payload: dto.payload as ScreenLayoutItem | ScreenLayoutItem[],
    });
    return this.components.save(c);
  }

  async removeComponent(id: number, ownerUserId: number): Promise<void> {
    const c = await this.components.findOne({
      where: { id, deletedAt: IsNull() },
    });
    if (!c) throw new NotFoundException('Component not found');
    if (c.ownerUserId !== ownerUserId) {
      throw new ForbiddenException('Not your component');
    }
    await this.components.softDelete(id);
  }
}
