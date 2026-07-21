import { BadRequestException, Injectable, NotFoundException } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { InjectRepository } from '@nestjs/typeorm';
import { unlink } from 'fs/promises';
import { join } from 'path';
import { IsNull, LessThan, Repository } from 'typeorm';
import { AppConfig } from '../config/configuration';
import { Device } from '../devices/device.entity';
import { Screenshot } from './screenshot.entity';

/** 디바이스별 보관할 최대 스크린샷 개수. */
const KEEP_PER_DEVICE = 10;
const SUBDIR = 'screenshots';

export interface UploadedScreenshotMeta {
  filename: string;
  mimetype: string;
  size: number;
}

@Injectable()
export class ScreenshotsService {
  constructor(
    @InjectRepository(Screenshot)
    private readonly screenshots: Repository<Screenshot>,
    @InjectRepository(Device) private readonly devices: Repository<Device>,
    private readonly config: ConfigService<AppConfig, true>,
  ) {}

  async createFromUpload(hardwareId: string, file: UploadedScreenshotMeta) {
    if (!file) throw new BadRequestException('No file uploaded');
    const device = await this.devices.findOne({
      where: { hardwareId, deletedAt: IsNull() },
    });
    if (!device) throw new NotFoundException('Device not registered');

    const shot = this.screenshots.create({
      deviceId: device.id,
      filePath: `${SUBDIR}/${file.filename}`,
      mimeType: file.mimetype,
      sizeBytes: file.size,
    });
    const saved = await this.screenshots.save(shot);

    await this.pruneOld(device.id);
    return { ok: true, id: saved.id };
  }

  async listForDevice(deviceId: number, limit = KEEP_PER_DEVICE) {
    const rows = await this.screenshots.find({
      where: { deviceId },
      order: { id: 'DESC' },
      take: Math.max(1, Math.min(limit, 100)),
    });
    return rows.map((s) => this.toView(s));
  }

  /** 최신 KEEP_PER_DEVICE개만 남기고 나머지는 파일 삭제 + 행 하드 삭제. */
  private async pruneOld(deviceId: number) {
    const newest = await this.screenshots.find({
      where: { deviceId },
      order: { id: 'DESC' },
      take: KEEP_PER_DEVICE,
      select: ['id'],
    });
    if (newest.length < KEEP_PER_DEVICE) return;
    const cutoffId = newest[newest.length - 1].id;
    const stale = await this.screenshots.find({
      where: { deviceId, id: LessThan(cutoffId) },
    });
    if (stale.length === 0) return;
    const dir = this.config.get('assets', { infer: true }).dir;
    for (const s of stale) {
      try {
        await unlink(join(dir, s.filePath));
      } catch {
        // 파일이 이미 없어도 무시
      }
    }
    await this.screenshots.remove(stale);
  }

  private toView(s: Screenshot) {
    const publicPath = this.config.get('assets', { infer: true }).publicPath;
    return {
      id: s.id,
      url: `${publicPath}/${s.filePath}`,
      sizeBytes: Number(s.sizeBytes),
      createdAt: s.createdAt,
    };
  }
}
