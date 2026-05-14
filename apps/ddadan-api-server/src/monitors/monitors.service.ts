import { ForbiddenException, Injectable, NotFoundException } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { IsNull, Repository } from 'typeorm';
import { Device } from '../devices/device.entity';
import { Screen } from '../screens/screen.entity';
import { StoresService } from '../stores/stores.service';
import { Monitor } from './monitor.entity';
import { SetMonitorRotationDto } from './dto/monitor.dto';

@Injectable()
export class MonitorsService {
  constructor(
    @InjectRepository(Monitor) private readonly monitors: Repository<Monitor>,
    @InjectRepository(Device) private readonly devices: Repository<Device>,
    @InjectRepository(Screen) private readonly screens: Repository<Screen>,
    private readonly stores: StoresService,
  ) {}

  async listForDevice(deviceId: number, ownerUserId: number) {
    const monitor = await this.assertOwnership(deviceId, ownerUserId);
    return this.monitors.find({
      where: { deviceId: monitor.deviceId, deletedAt: IsNull() },
      order: { slot: 'ASC' },
    });
  }

  async updatePosition(
    monitorId: number,
    ownerUserId: number,
    positionX: number,
    positionY: number,
  ) {
    const monitor = await this.findOwned(monitorId, ownerUserId);
    monitor.positionX = positionX;
    monitor.positionY = positionY;
    return this.monitors.save(monitor);
  }

  async assignScreen(
    monitorId: number,
    ownerUserId: number,
    screenId: number | null,
  ) {
    const monitor = await this.findOwned(monitorId, ownerUserId);
    if (screenId !== null) {
      const screen = await this.screens.findOne({
        where: { id: screenId, deletedAt: IsNull() },
      });
      if (!screen) throw new NotFoundException('Screen not found');
      if (screen.ownerUserId !== ownerUserId) {
        throw new ForbiddenException('Not your screen');
      }
    }
    monitor.currentScreenId = screenId;
    monitor.rotationScreenIdsJson = null;
    return this.monitors.save(monitor);
  }

  async setRotation(
    monitorId: number,
    ownerUserId: number,
    dto: SetMonitorRotationDto,
  ) {
    const monitor = await this.findOwned(monitorId, ownerUserId);
    const seen = new Set<number>();
    const ids: number[] = [];
    for (const sid of dto.screenIds) {
      if (!seen.has(sid)) {
        seen.add(sid);
        ids.push(sid);
      }
    }

    for (const sid of ids) {
      const screen = await this.screens.findOne({
        where: { id: sid, deletedAt: IsNull() },
      });
      if (!screen) throw new NotFoundException(`Screen ${sid} not found`);
      if (screen.ownerUserId !== ownerUserId) {
        throw new ForbiddenException('Not your screen');
      }
    }

    monitor.rotationIntervalMs = dto.intervalMs;
    monitor.rotationFadeMs = dto.fadeMs;

    if (ids.length === 0) {
      monitor.rotationScreenIdsJson = null;
      monitor.currentScreenId = null;
      return this.monitors.save(monitor);
    }

    if (ids.length === 1) {
      monitor.rotationScreenIdsJson = null;
      monitor.currentScreenId = ids[0];
      return this.monitors.save(monitor);
    }

    monitor.rotationScreenIdsJson = JSON.stringify(ids);
    monitor.currentScreenId = ids[0];
    return this.monitors.save(monitor);
  }

  async findByDeviceAndSlot(deviceId: number, slot: number) {
    return this.monitors.findOne({
      where: { deviceId, slot, deletedAt: IsNull() },
    });
  }

  private async findOwned(monitorId: number, ownerUserId: number) {
    const monitor = await this.monitors.findOne({
      where: { id: monitorId, deletedAt: IsNull() },
    });
    if (!monitor) throw new NotFoundException('Monitor not found');
    await this.assertOwnership(monitor.deviceId, ownerUserId);
    return monitor;
  }

  private async assertOwnership(deviceId: number, ownerUserId: number) {
    const device = await this.devices.findOne({
      where: { id: deviceId, deletedAt: IsNull() },
    });
    if (!device) throw new NotFoundException('Device not found');
    if (!device.storeId) throw new ForbiddenException('Device unregistered');
    await this.stores.getOwned(device.storeId, ownerUserId);
    return { deviceId };
  }
}
