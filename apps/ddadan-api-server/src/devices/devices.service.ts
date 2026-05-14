import {
  ConflictException,
  ForbiddenException,
  Injectable,
  NotFoundException,
} from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { InjectRepository } from '@nestjs/typeorm';
import { IsNull, Repository } from 'typeorm';
import { AppConfig } from '../config/configuration';
import { Monitor } from '../monitors/monitor.entity';
import { StoresService } from '../stores/stores.service';
import { Device, DeviceStatus } from './device.entity';
import { HeartbeatDto, MonitorReportDto, RegisterDeviceDto } from './dto/device.dto';

@Injectable()
export class DevicesService {
  constructor(
    @InjectRepository(Device) private readonly devices: Repository<Device>,
    @InjectRepository(Monitor) private readonly monitors: Repository<Monitor>,
    private readonly stores: StoresService,
    private readonly config: ConfigService<AppConfig, true>,
  ) {}

  async checkByHardwareId(hardwareId: string, monitors?: MonitorReportDto[]) {
    const device = await this.devices.findOne({
      where: { hardwareId, deletedAt: IsNull() },
    });
    if (!device) {
      return { registered: false as const };
    }
    if (monitors?.length) {
      await this.syncMonitors(device.id, monitors);
    }
    return {
      registered: true as const,
      device: await this.toView(device),
    };
  }

  async listAllForOwner(ownerUserId: number) {
    const rows = await this.devices
      .createQueryBuilder('d')
      .innerJoinAndSelect('d.store', 's')
      .where('s.ownerUserId = :ownerUserId', { ownerUserId })
      .andWhere('d.deletedAt IS NULL')
      .andWhere('s.deletedAt IS NULL')
      .orderBy('d.id', 'ASC')
      .getMany();
    return Promise.all(rows.map((d) => this.toView(d)));
  }

  async listForStore(storeId: number, ownerUserId: number) {
    await this.stores.getOwned(storeId, ownerUserId);
    const devices = await this.devices.find({
      where: { storeId, deletedAt: IsNull() },
      order: { id: 'ASC' },
    });
    return Promise.all(devices.map((d) => this.toView(d)));
  }

  async getOwned(deviceId: number, ownerUserId: number): Promise<Device> {
    const device = await this.devices.findOne({
      where: { id: deviceId, deletedAt: IsNull() },
    });
    if (!device) throw new NotFoundException('Device not found');
    if (!device.storeId) throw new ForbiddenException('Device unregistered');
    await this.stores.getOwned(device.storeId, ownerUserId);
    return device;
  }

  /** Single-device view for admin detail (avoids list.find id strict-equality pitfalls). */
  async getViewForOwner(deviceId: number, ownerUserId: number) {
    const device = await this.devices.findOne({
      where: { id: deviceId, deletedAt: IsNull() },
      relations: ['store'],
    });
    if (!device) throw new NotFoundException('Device not found');
    if (!device.storeId) throw new ForbiddenException('Device unregistered');
    await this.stores.getOwned(device.storeId, ownerUserId);
    return this.toView(device);
  }

  async register(
    ownerUserId: number,
    dto: RegisterDeviceDto,
  ): Promise<Device> {
    await this.stores.getOwned(dto.storeId, ownerUserId);

    let device = await this.devices.findOne({
      where: { hardwareId: dto.hardwareId, deletedAt: IsNull() },
    });
    if (device && device.storeId && device.storeId !== dto.storeId) {
      throw new ConflictException('Device is already registered to another store');
    }
    if (!device) {
      device = this.devices.create({
        hardwareId: dto.hardwareId,
        storeId: dto.storeId,
        name: dto.name ?? null,
        status: 'online',
      });
    } else {
      device.storeId = dto.storeId;
      device.name = dto.name ?? device.name ?? null;
      device.status = 'online';
    }
    device.lastSeenAt = new Date();
    device = await this.devices.save(device);

    if (dto.monitors?.length) {
      await this.syncMonitors(device.id, dto.monitors);
    } else {
      await this.ensureDefaultMonitor(device.id);
    }
    return device;
  }

  async heartbeat(dto: HeartbeatDto) {
    const device = await this.devices.findOne({
      where: { hardwareId: dto.hardwareId, deletedAt: IsNull() },
    });
    if (!device) throw new NotFoundException('Device not registered');

    device.status = 'online';
    device.lastSeenAt = new Date();
    if (dto.appVersion !== undefined) device.appVersion = dto.appVersion;
    await this.devices.save(device);

    if (dto.monitors?.length) {
      await this.syncMonitors(device.id, dto.monitors);
    }

    return { ok: true, deviceId: device.id };
  }

  async unregister(deviceId: number, ownerUserId: number) {
    const device = await this.getOwned(deviceId, ownerUserId);
    await this.devices.softDelete(device.id);
  }

  async rename(deviceId: number, ownerUserId: number, name: string) {
    const device = await this.getOwned(deviceId, ownerUserId);
    device.name = name;
    return this.devices.save(device);
  }

  async markOfflineByExpiry(deviceId: number) {
    const device = await this.devices.findOne({
      where: { id: deviceId, deletedAt: IsNull() },
    });
    if (!device || device.status === 'offline') return;
    device.status = 'offline';
    await this.devices.save(device);
  }

  async listAllRegistered(): Promise<Device[]> {
    return this.devices.find({
      where: { deletedAt: IsNull() },
    });
  }

  /** Public for HeartbeatService — same rule as API list views. */
  static liveStatusFromLastSeen(
    lastSeenAt: Date | null,
    offlineAfterSeconds: number,
    persistedStatus: DeviceStatus,
  ): DeviceStatus {
    if (!lastSeenAt) return persistedStatus;
    const ageMs = Date.now() - lastSeenAt.getTime();
    return ageMs < offlineAfterSeconds * 1000 ? 'online' : 'offline';
  }

  private async ensureDefaultMonitor(deviceId: number) {
    const existing = await this.monitors.find({
      where: { deviceId, deletedAt: IsNull() },
    });
    if (existing.length === 0) {
      const m = this.monitors.create({
        deviceId,
        slot: 0,
        resolutionW: 1920,
        resolutionH: 1080,
        positionX: 0,
        positionY: 0,
      });
      await this.monitors.save(m);
    }
  }

  private async syncMonitors(deviceId: number, reports: MonitorReportDto[]) {
    for (const r of reports) {
      const found = await this.monitors.findOne({
        where: { deviceId, slot: r.slot, deletedAt: IsNull() },
      });
      if (found) {
        if (
          found.resolutionW !== r.resolutionW ||
          found.resolutionH !== r.resolutionH
        ) {
          found.resolutionW = r.resolutionW;
          found.resolutionH = r.resolutionH;
          await this.monitors.save(found);
        }
      } else {
        const created = this.monitors.create({
          deviceId,
          slot: r.slot,
          resolutionW: r.resolutionW,
          resolutionH: r.resolutionH,
          positionX: r.slot * r.resolutionW,
          positionY: 0,
        });
        await this.monitors.save(created);
      }
    }
  }

  private async toView(device: Device) {
    const monitorsRaw = await this.monitors.find({
      where: { deviceId: device.id, deletedAt: IsNull() },
      order: { slot: 'ASC' },
    });
    const monitors = monitorsRaw.map((m) => {
      let rotationScreenIds: number[] = [];
      if (m.rotationScreenIdsJson) {
        try {
          const parsed = JSON.parse(m.rotationScreenIdsJson) as unknown;
          if (Array.isArray(parsed)) {
            rotationScreenIds = parsed.filter((x): x is number => typeof x === 'number');
          }
        } catch {
          rotationScreenIds = [];
        }
      }
      return {
        id: m.id,
        deviceId: m.deviceId,
        slot: m.slot,
        resolutionW: m.resolutionW,
        resolutionH: m.resolutionH,
        positionX: m.positionX,
        positionY: m.positionY,
        currentScreenId: m.currentScreenId,
        rotationScreenIds,
        rotationIntervalMs: m.rotationIntervalMs ?? 10_000,
        rotationFadeMs: m.rotationFadeMs ?? 800,
      };
    });
    const ttl = this.config.get('heartbeat', { infer: true }).offlineAfterSeconds;
    const liveStatus = DevicesService.liveStatusFromLastSeen(
      device.lastSeenAt,
      ttl,
      device.status,
    );
    return {
      id: device.id,
      hardwareId: device.hardwareId,
      storeId: device.storeId,
      storeName: device.store?.name ?? null,
      name: device.name,
      status: liveStatus,
      lastSeenAt: device.lastSeenAt,
      appVersion: device.appVersion,
      offlineAfterSeconds: ttl,
      monitors,
    };
  }
}
