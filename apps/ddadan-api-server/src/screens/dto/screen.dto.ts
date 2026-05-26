import { Type } from 'class-transformer';
import {
  Allow,
  IsArray,
  IsIn,
  IsInt,
  IsNumber,
  IsOptional,
  IsString,
  Length,
  Min,
  ValidateNested,
} from 'class-validator';

export class ScreenLayoutItemDto {
  @IsString()
  id!: string;

  @IsIn(['image', 'video', 'text'])
  kind!: 'image' | 'video' | 'text';

  @IsOptional()
  @IsInt()
  assetId?: number | null;

  @IsOptional()
  @IsInt()
  componentId?: number | null;

  @IsOptional()
  @IsString()
  text?: string;

  @IsOptional()
  @IsNumber()
  fontSize?: number;

  @IsOptional()
  @IsString()
  color?: string;

  @IsOptional()
  @IsString()
  background?: string;

  @IsNumber()
  x!: number;

  @IsNumber()
  y!: number;

  @IsNumber()
  width!: number;

  @IsNumber()
  height!: number;

  @IsOptional()
  @IsNumber()
  zIndex?: number;

  @IsOptional()
  @IsNumber()
  durationMs?: number;
}

export class ScreenLayoutDto {
  @IsOptional()
  @IsString()
  background?: string;

  @IsArray()
  @ValidateNested({ each: true })
  @Type(() => ScreenLayoutItemDto)
  items!: ScreenLayoutItemDto[];
}

export class CreateScreenDto {
  @IsString()
  @Length(1, 200)
  name!: string;

  @IsOptional()
  @IsInt()
  storeId?: number;

  @IsInt()
  @Min(1)
  width!: number;

  @IsInt()
  @Min(1)
  height!: number;

  @ValidateNested()
  @Type(() => ScreenLayoutDto)
  layout!: ScreenLayoutDto;
}

export class UpdateScreenDto {
  @IsOptional()
  @IsString()
  @Length(1, 200)
  name?: string;

  @IsOptional()
  @IsInt()
  @Min(1)
  width?: number;

  @IsOptional()
  @IsInt()
  @Min(1)
  height?: number;

  @IsOptional()
  @ValidateNested()
  @Type(() => ScreenLayoutDto)
  layout?: ScreenLayoutDto;
}

export class CreateScreenComponentDto {
  @IsString()
  @Length(1, 200)
  name!: string;

  @IsIn(['image', 'video', 'text', 'group'])
  kind!: 'image' | 'video' | 'text' | 'group';

  @Allow()
  payload!: unknown;
}
