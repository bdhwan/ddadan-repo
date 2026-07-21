import {
  BadRequestException,
  Controller,
  Get,
  Param,
  ParseIntPipe,
  Post,
  Query,
  UploadedFile,
  UseInterceptors,
} from '@nestjs/common';
import { FileInterceptor } from '@nestjs/platform-express';
import { ScreenshotsService } from './screenshots.service';

@Controller('devices')
export class ScreenshotsController {
  constructor(private readonly screenshots: ScreenshotsService) {}

  /** 플레이어(디바이스)가 주기적으로 현재 화면을 업로드. hardwareId로 식별. */
  @Post(':hardwareId/screenshots')
  @UseInterceptors(FileInterceptor('file', { limits: { fileSize: 20 * 1024 * 1024 } }))
  async upload(
    @Param('hardwareId') hardwareId: string,
    @UploadedFile() file: Express.Multer.File,
  ) {
    if (!file) throw new BadRequestException('No file uploaded');
    return this.screenshots.createFromUpload(hardwareId, {
      filename: file.filename,
      mimetype: file.mimetype,
      size: file.size,
    });
  }

  /** Admin: 디바이스(numeric id)의 최근 스크린샷 목록(최신순). */
  @Get(':id/screenshots')
  async list(
    @Param('id', ParseIntPipe) id: number,
    @Query('limit') limitRaw?: string,
  ) {
    const limit = limitRaw ? Number(limitRaw) : 10;
    return this.screenshots.listForDevice(id, limit);
  }
}
