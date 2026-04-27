import {
  BadRequestException,
  ForbiddenException,
  Injectable,
  NotFoundException,
} from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { InjectRepository } from '@nestjs/typeorm';
import { unlink } from 'fs/promises';
import { extname, join } from 'path';
import { IsNull, Repository } from 'typeorm';
import { AppConfig } from '../config/configuration';
import { StoresService } from '../stores/stores.service';
import { Asset, AssetType } from './asset.entity';
import { CreateTextAssetDto, UpdateAssetDto } from './dto/asset.dto';

const IMAGE_MIMES = new Set([
  'image/png',
  'image/jpeg',
  'image/jpg',
  'image/gif',
  'image/webp',
]);
const VIDEO_MIMES = new Set([
  'video/mp4',
  'video/webm',
  'video/quicktime',
  'video/ogg',
]);

export interface UploadedAssetMeta {
  filename: string;
  originalname: string;
  mimetype: string;
  size: number;
}

@Injectable()
export class AssetsService {
  constructor(
    @InjectRepository(Asset) private readonly assets: Repository<Asset>,
    private readonly stores: StoresService,
    private readonly config: ConfigService<AppConfig, true>,
  ) {}

  list(
    ownerUserId: number,
    filters: { type?: AssetType; storeId?: number } = {},
  ) {
    const where: Record<string, unknown> = {
      ownerUserId,
      deletedAt: IsNull(),
    };
    if (filters.type) where.type = filters.type;
    if (filters.storeId) where.storeId = filters.storeId;
    return this.assets.find({ where, order: { id: 'DESC' } });
  }

  async createFromUpload(
    ownerUserId: number,
    file: UploadedAssetMeta,
    storeId?: number,
  ): Promise<Asset> {
    if (!file) throw new BadRequestException('No file uploaded');

    let type: AssetType;
    if (IMAGE_MIMES.has(file.mimetype)) type = 'image';
    else if (VIDEO_MIMES.has(file.mimetype)) type = 'video';
    else throw new BadRequestException(`Unsupported mime type: ${file.mimetype}`);

    if (storeId) {
      await this.stores.getOwned(storeId, ownerUserId);
    }

    const asset = this.assets.create({
      ownerUserId,
      storeId: storeId ?? null,
      type,
      originalName: file.originalname,
      filePath: file.filename,
      mimeType: file.mimetype,
      sizeBytes: file.size,
    });
    return this.assets.save(asset);
  }

  async createText(
    ownerUserId: number,
    dto: CreateTextAssetDto,
  ): Promise<Asset> {
    if (dto.storeId) {
      await this.stores.getOwned(dto.storeId, ownerUserId);
    }
    const asset = this.assets.create({
      ownerUserId,
      storeId: dto.storeId ?? null,
      type: 'text',
      originalName: dto.originalName,
      textContent: dto.textContent,
      sizeBytes: Buffer.byteLength(dto.textContent, 'utf8'),
    });
    return this.assets.save(asset);
  }

  async update(id: number, ownerUserId: number, dto: UpdateAssetDto) {
    const asset = await this.findOwned(id, ownerUserId);
    if (dto.originalName !== undefined) asset.originalName = dto.originalName;
    if (dto.textContent !== undefined && asset.type === 'text') {
      asset.textContent = dto.textContent;
      asset.sizeBytes = Buffer.byteLength(dto.textContent, 'utf8');
    }
    return this.assets.save(asset);
  }

  async remove(id: number, ownerUserId: number) {
    const asset = await this.findOwned(id, ownerUserId);
    await this.assets.softDelete(id);
    if (asset.filePath) {
      const dir = this.config.get('assets', { infer: true }).dir;
      try {
        await unlink(join(dir, asset.filePath));
      } catch {
        // ignore
      }
    }
  }

  toView(asset: Asset) {
    const publicPath = this.config.get('assets', { infer: true }).publicPath;
    return {
      id: asset.id,
      type: asset.type,
      originalName: asset.originalName,
      mimeType: asset.mimeType,
      sizeBytes: Number(asset.sizeBytes),
      url: asset.filePath ? `${publicPath}/${asset.filePath}` : null,
      textContent: asset.textContent,
      storeId: asset.storeId,
      createdAt: asset.createdAt,
    };
  }

  private async findOwned(id: number, ownerUserId: number) {
    const asset = await this.assets.findOne({
      where: { id, deletedAt: IsNull() },
    });
    if (!asset) throw new NotFoundException('Asset not found');
    if (asset.ownerUserId !== ownerUserId) {
      throw new ForbiddenException('Not your asset');
    }
    return asset;
  }

  static buildFilename(originalName: string): string {
    const ext = extname(originalName).toLowerCase().slice(0, 10);
    const stamp = Date.now();
    const rand = Math.random().toString(36).slice(2, 10);
    return `${stamp}-${rand}${ext}`;
  }
}
