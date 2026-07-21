import { BadRequestException, Injectable } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { InjectRepository } from '@nestjs/typeorm';
import { Repository } from 'typeorm';
import { AppConfig } from '../config/configuration';
import { Apk } from './apk.entity';

const SUBDIR = 'apks';

export interface UploadedApkMeta {
  filename: string;
  size: number;
}

export interface CreateApkInput {
  versionCode: number;
  versionName?: string;
  applicationId?: string;
}

@Injectable()
export class ApksService {
  constructor(
    @InjectRepository(Apk) private readonly apks: Repository<Apk>,
    private readonly config: ConfigService<AppConfig, true>,
  ) {}

  async createFromUpload(file: UploadedApkMeta, input: CreateApkInput) {
    if (!file) throw new BadRequestException('No file uploaded');
    if (!Number.isFinite(input.versionCode) || input.versionCode <= 0) {
      throw new BadRequestException('versionCode is required');
    }
    const apk = this.apks.create({
      versionCode: Math.trunc(input.versionCode),
      versionName: input.versionName ?? null,
      applicationId: input.applicationId ?? null,
      filePath: `${SUBDIR}/${file.filename}`,
      sizeBytes: file.size,
    });
    const saved = await this.apks.save(apk);
    return this.toView(saved);
  }

  async latest(applicationId?: string) {
    const rows = await this.apks.find({
      where: applicationId ? { applicationId } : {},
      order: { versionCode: 'DESC', id: 'DESC' },
      take: 1,
    });
    return rows.length ? this.toView(rows[0]) : null;
  }

  async list() {
    const rows = await this.apks.find({
      order: { versionCode: 'DESC', id: 'DESC' },
    });
    return rows.map((a) => this.toView(a));
  }

  private toView(apk: Apk) {
    const publicPath = this.config.get('assets', { infer: true }).publicPath;
    return {
      id: apk.id,
      versionCode: apk.versionCode,
      versionName: apk.versionName,
      applicationId: apk.applicationId,
      url: `${publicPath}/${apk.filePath}`,
      sizeBytes: Number(apk.sizeBytes),
      createdAt: apk.createdAt,
    };
  }
}
