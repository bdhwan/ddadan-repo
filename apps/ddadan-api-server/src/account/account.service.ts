import { Injectable, Logger } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { DataSource, IsNull, Repository } from 'typeorm';
import { Asset } from '../assets/asset.entity';
import { Device } from '../devices/device.entity';
import { Monitor } from '../monitors/monitor.entity';
import { ScreenComponent } from '../screens/screen-component.entity';
import { Screen } from '../screens/screen.entity';
import { Store } from '../stores/store.entity';
import { User } from '../users/user.entity';

@Injectable()
export class AccountService {
  private readonly logger = new Logger(AccountService.name);

  constructor(
    @InjectRepository(User) private readonly users: Repository<User>,
    @InjectRepository(Store) private readonly stores: Repository<Store>,
    @InjectRepository(Device) private readonly devices: Repository<Device>,
    @InjectRepository(Monitor) private readonly monitors: Repository<Monitor>,
    @InjectRepository(Asset) private readonly assets: Repository<Asset>,
    @InjectRepository(Screen) private readonly screens: Repository<Screen>,
    @InjectRepository(ScreenComponent)
    private readonly components: Repository<ScreenComponent>,
    private readonly dataSource: DataSource,
  ) {}

  async withdraw(userId: number) {
    await this.dataSource.transaction(async (manager) => {
      const storeIds = (
        await manager.find(Store, { where: { ownerUserId: userId, deletedAt: IsNull() } })
      ).map((s) => s.id);

      if (storeIds.length) {
        const deviceIds = (
          await manager.find(Device, {
            where: storeIds.map((id) => ({ storeId: id, deletedAt: IsNull() })),
          })
        ).map((d) => d.id);
        if (deviceIds.length) {
          await manager.softDelete(Monitor, { deviceId: deviceIds as any });
          await manager.softDelete(Device, { id: deviceIds as any });
        }
        await manager.softDelete(Store, { id: storeIds as any });
      }

      await manager.softDelete(Asset, { ownerUserId: userId });
      await manager.softDelete(Screen, { ownerUserId: userId });
      await manager.softDelete(ScreenComponent, { ownerUserId: userId });
    });

    this.logger.log(`Local user ${userId} data withdrawn (soft delete)`);
  }
}
