import {
  Controller,
  Get,
  NotFoundException,
  Param,
  Query,
} from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { InjectRepository } from '@nestjs/typeorm';
import { IsNull, Repository } from 'typeorm';
import { AppConfig } from '../config/configuration';
import { Asset } from '../assets/asset.entity';
import { Device } from '../devices/device.entity';
import { Monitor } from '../monitors/monitor.entity';
import { Screen, ScreenLayoutItem } from '../screens/screen.entity';

@Controller('player')
export class PlayerController {
  constructor(
    @InjectRepository(Device) private readonly devices: Repository<Device>,
    @InjectRepository(Monitor) private readonly monitors: Repository<Monitor>,
    @InjectRepository(Screen) private readonly screens: Repository<Screen>,
    @InjectRepository(Asset) private readonly assets: Repository<Asset>,
    private readonly config: ConfigService<AppConfig, true>,
  ) {}

  @Get(':hardwareId/screen')
  async getScreen(
    @Param('hardwareId') hardwareId: string,
    @Query('slot') slotRaw = '0',
  ) {
    const slot = Number(slotRaw);
    const device = await this.devices.findOne({
      where: { hardwareId, deletedAt: IsNull() },
    });
    if (!device) {
      return this.fallbackInfoScreen({ hardwareId, slot, registered: false });
    }

    const monitor = await this.monitors.findOne({
      where: { deviceId: device.id, slot, deletedAt: IsNull() },
    });
    if (!monitor) {
      return this.fallbackInfoScreen({
        hardwareId,
        slot,
        registered: true,
        deviceName: device.name,
        resolutionW: 1920,
        resolutionH: 1080,
      });
    }

    const rotationIds = this.parseRotationIds(monitor.rotationScreenIdsJson);
    if (rotationIds.length >= 2) {
      const loaded = await Promise.all(
        rotationIds.map((id) =>
          this.screens.findOne({ where: { id, deletedAt: IsNull() } }),
        ),
      );
      if (loaded.some((s) => !s)) {
        throw new NotFoundException('Assigned screen missing');
      }
      const slides = await Promise.all(
        loaded.map((sc) => this.buildSlidePayload(sc!)),
      );
      const base = slides[0];
      return {
        registered: true,
        deviceName: device.name,
        mode: 'rotation' as const,
        width: base.width,
        height: base.height,
        background: base.background,
        items: base.items,
        rotation: {
          intervalMs: monitor.rotationIntervalMs ?? 10_000,
          fadeMs: monitor.rotationFadeMs ?? 800,
          slides,
        },
      };
    }

    const activeId =
      rotationIds.length === 1 ? rotationIds[0] : monitor.currentScreenId;
    if (activeId == null) {
      return this.fallbackInfoScreen({
        hardwareId,
        slot,
        registered: true,
        deviceName: device.name,
        resolutionW: monitor.resolutionW ?? 1920,
        resolutionH: monitor.resolutionH ?? 1080,
      });
    }

    const screen = await this.screens.findOne({
      where: { id: activeId, deletedAt: IsNull() },
    });
    if (!screen) {
      throw new NotFoundException('Assigned screen missing');
    }

    const payload = await this.buildSlidePayload(screen);
    return {
      registered: true,
      deviceName: device.name,
      mode: 'single' as const,
      width: payload.width,
      height: payload.height,
      background: payload.background,
      items: payload.items,
    };
  }

  private parseRotationIds(json: string | null): number[] {
    if (!json) return [];
    try {
      const parsed = JSON.parse(json) as unknown;
      if (!Array.isArray(parsed)) return [];
      return parsed.filter((x): x is number => typeof x === 'number');
    } catch {
      return [];
    }
  }

  private async buildSlidePayload(screen: Screen) {
    const items = await this.expandLayout(screen.layout.items);
    return {
      width: screen.width,
      height: screen.height,
      background: screen.layout.background,
      items,
    };
  }

  private async expandLayout(items: ScreenLayoutItem[]) {
    const assetIds = items
      .map((i) => i.assetId)
      .filter((v): v is number => typeof v === 'number');
    const assetMap = new Map<number, Asset>();
    if (assetIds.length) {
      const found = await this.assets.find({
        where: assetIds.map((id) => ({ id, deletedAt: IsNull() })),
      });
      for (const a of found) assetMap.set(a.id, a);
    }
    const publicPath = this.config.get('assets', { infer: true }).publicPath;
    return items.map((item) => {
      const out: Record<string, unknown> = { ...item };
      if (item.assetId && assetMap.has(item.assetId)) {
        const asset = assetMap.get(item.assetId)!;
        if (asset.filePath) out.url = `${publicPath}/${asset.filePath}`;
        if (asset.type === 'text' && asset.textContent != null) {
          out.text = asset.textContent;
        }
      }
      return out;
    });
  }

  private fallbackInfoScreen(info: {
    hardwareId: string;
    slot: number;
    registered: boolean;
    deviceName?: string | null;
    resolutionW?: number;
    resolutionH?: number;
  }) {
    return {
      registered: info.registered,
      deviceName: info.deviceName ?? null,
      mode: 'single' as const,
      width: info.resolutionW ?? 1920,
      height: info.resolutionH ?? 1080,
      background: '#0c0f1a',
      items: [
        {
          id: 'info',
          kind: 'text',
          x: 0,
          y: 0,
          width: info.resolutionW ?? 1920,
          height: info.resolutionH ?? 1080,
          color: '#ffffff',
          fontSize: 48,
          text: info.registered
            ? `DDADAN\n${info.deviceName ?? info.hardwareId} · slot ${info.slot}\n화면이 아직 할당되지 않았습니다`
            : `DDADAN 등록이 필요합니다\n등록 코드: ${info.hardwareId}`,
        },
      ],
      isFallback: true,
    };
  }
}
