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
import { UsersService } from '../users/users.service';
import { AssetsService } from './assets.service';
import { AssetQueryDto, CreateTextAssetDto, UpdateAssetDto } from './dto/asset.dto';

@Controller('assets')
export class AssetsController {
  constructor(
    private readonly assets: AssetsService,
    private readonly users: UsersService,
  ) {}

  @Get()
  async list(@Query() query: AssetQueryDto) {
    const list = await this.assets.list(this.users.getLocalUserId(), query);
    return list.map((a) => this.assets.toView(a));
  }

  @Post('upload')
  @UseInterceptors(FileInterceptor('file', { limits: { fileSize: 200 * 1024 * 1024 } }))
  async upload(
    @UploadedFile() file: Express.Multer.File,
    @Body('storeId') storeIdRaw?: string,
  ) {
    if (!file) throw new BadRequestException('No file uploaded');
    const storeId = storeIdRaw ? Number(storeIdRaw) : undefined;
    const asset = await this.assets.createFromUpload(
      this.users.getLocalUserId(),
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
  async createText(@Body() dto: CreateTextAssetDto) {
    const asset = await this.assets.createText(this.users.getLocalUserId(), dto);
    return this.assets.toView(asset);
  }

  @Patch(':id')
  async update(
    @Param('id', ParseIntPipe) id: number,
    @Body() dto: UpdateAssetDto,
  ) {
    const asset = await this.assets.update(
      id,
      this.users.getLocalUserId(),
      dto,
    );
    return this.assets.toView(asset);
  }

  @Delete(':id')
  @HttpCode(204)
  async remove(@Param('id', ParseIntPipe) id: number) {
    await this.assets.remove(id, this.users.getLocalUserId());
  }
}
