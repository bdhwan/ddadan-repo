import {
  BadRequestException,
  Body,
  Controller,
  Delete,
  Get,
  HttpCode,
  Param,
  ParseIntPipe,
  Patch,
  Post,
  Query,
  UploadedFile,
  UseInterceptors,
} from '@nestjs/common';
import { FileInterceptor } from '@nestjs/platform-express';
import { CurrentUser } from '../auth/current-user.decorator';
import type { AuthContext } from '../auth/firebase-auth.guard';
import { AssetsService } from './assets.service';
import { AssetQueryDto, CreateTextAssetDto, UpdateAssetDto } from './dto/asset.dto';

@Controller('assets')
export class AssetsController {
  constructor(private readonly assets: AssetsService) {}

  @Get()
  async list(
    @CurrentUser() auth: AuthContext,
    @Query() query: AssetQueryDto,
  ) {
    const list = await this.assets.list(auth.userId, query);
    return list.map((a) => this.assets.toView(a));
  }

  @Post('upload')
  @UseInterceptors(FileInterceptor('file', { limits: { fileSize: 200 * 1024 * 1024 } }))
  async upload(
    @CurrentUser() auth: AuthContext,
    @UploadedFile() file: Express.Multer.File,
    @Body('storeId') storeIdRaw?: string,
  ) {
    if (!file) throw new BadRequestException('No file uploaded');
    const storeId = storeIdRaw ? Number(storeIdRaw) : undefined;
    const asset = await this.assets.createFromUpload(
      auth.userId,
      {
        filename: file.filename,
        originalname: file.originalname,
        mimetype: file.mimetype,
        size: file.size,
      },
      storeId,
    );
    return this.assets.toView(asset);
  }

  @Post('text')
  async createText(
    @CurrentUser() auth: AuthContext,
    @Body() dto: CreateTextAssetDto,
  ) {
    const asset = await this.assets.createText(auth.userId, dto);
    return this.assets.toView(asset);
  }

  @Patch(':id')
  async update(
    @CurrentUser() auth: AuthContext,
    @Param('id', ParseIntPipe) id: number,
    @Body() dto: UpdateAssetDto,
  ) {
    const asset = await this.assets.update(id, auth.userId, dto);
    return this.assets.toView(asset);
  }

  @Delete(':id')
  @HttpCode(204)
  async remove(
    @CurrentUser() auth: AuthContext,
    @Param('id', ParseIntPipe) id: number,
  ) {
    await this.assets.remove(id, auth.userId);
  }
}
