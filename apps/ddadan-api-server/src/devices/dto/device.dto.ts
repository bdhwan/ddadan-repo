import { Type } from 'class-transformer';
import {
  ArrayMaxSize,
  ArrayMinSize,
  IsArray,
  IsInt,
  IsNumber,
  IsOptional,
  IsString,
  Length,
  Min,
  ValidateNested,
} from 'class-validator';

export class MonitorReportDto {
  @IsInt()
  @Min(0)
  slot!: number;

  @IsInt()
  @Min(1)
  resolutionW!: number;

  @IsInt()
  @Min(1)
  resolutionH!: number;
}

export class RegisterDeviceDto {
  @IsString()
  @Length(1, 128)
  hardwareId!: string;

  @IsInt()
  storeId!: number;

  @IsOptional()
  @IsString()
  @Length(1, 200)
  name?: string;

  @IsOptional()
  @IsArray()
  @ArrayMinSize(0)
  @ArrayMaxSize(2)
  @ValidateNested({ each: true })
  @Type(() => MonitorReportDto)
  monitors?: MonitorReportDto[];
}

export class CheckDeviceDto {
  @IsString()
  @Length(1, 128)
  hardwareId!: string;

  @IsOptional()
  @IsArray()
  @ArrayMaxSize(2)
  @ValidateNested({ each: true })
  @Type(() => MonitorReportDto)
  monitors?: MonitorReportDto[];
}

export class UpdateDeviceDto {
  @IsOptional()
  @IsString()
  @Length(1, 200)
  name?: string;
}

export class HeartbeatDto {
  @IsString()
  @Length(1, 128)
  hardwareId!: string;

  @IsOptional()
  @IsString()
  @Length(0, 64)
  appVersion?: string;

  @IsOptional()
  @IsArray()
  @ArrayMaxSize(2)
  @ValidateNested({ each: true })
  @Type(() => MonitorReportDto)
  monitors?: MonitorReportDto[];
}

export class TelemetryDto {
  @IsOptional()
  @IsString()
  @Length(0, 64)
  appVersion?: string;

  @IsOptional()
  @IsString()
  @Length(0, 64)
  watchdogVersion?: string;

  @IsOptional()
  @IsNumber()
  cpuPercent?: number;

  @IsOptional()
  @IsInt()
  ramUsedMb?: number;

  @IsOptional()
  @IsInt()
  ramTotalMb?: number;

  @IsOptional()
  @IsNumber()
  diskUsedPercent?: number;
}
