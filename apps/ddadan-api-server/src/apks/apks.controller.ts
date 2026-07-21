import {
  BadRequestException,
  Body,
  Controller,
  Get,
  Post,
  UploadedFile,
  UseInterceptors,
} from '@nestjs/common';
import { FileInterceptor } from '@nestjs/platform-express';
import { ApksService } from './apks.service';

@Controller('apks')
export class ApksController {
  constructor(private readonly apks: ApksService) {}

  /** 새 APK 업로드. versionCode(필수)/versionName/applicationId는 폼 필드로 전달. */
  @Post('upload')
  @UseInterceptors(FileInterceptor('file', { limits: { fileSize: 200 * 1024 * 1024 } }))
  async upload(
    @UploadedFile() file: Express.Multer.File,
    @Body('versionCode') versionCodeRaw?: string,
    @Body('versionName') versionName?: string,
    @Body('applicationId') applicationId?: string,
  ) {
    if (!file) throw new BadRequestException('No file uploaded');
    return this.apks.createFromUpload(
      { filename: file.filename, size: file.size },
      {
        versionCode: Number(versionCodeRaw),
        versionName,
        applicationId,
      },
    );
  }

  /** 최신 APK. 없으면 versionCode 0(=업데이트 없음)을 돌려준다. */
  @Get('latest')
  async latest() {
    return (
      (await this.apks.latest()) ?? {
        versionCode: 0,
        versionName: null,
        applicationId: null,
        url: null,
        sizeBytes: 0,
      }
    );
  }

  @Get()
  async list() {
    return this.apks.list();
  }
}
