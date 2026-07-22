import { Type } from 'class-transformer';
import {
  Allow,
  ArrayMaxSize,
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

/** 메뉴 행/헤더에 붙는 인라인 뱃지(BEST/추천/ICED Only/DECAF 등). */
export class BadgeDto {
  @IsString()
  @Length(1, 24)
  text!: string;

  /** 색/모양 프리셋. best=파란원, rec=초록원, info=회색 이탤릭, warn=초록박스. */
  @IsOptional()
  @IsIn(['best', 'rec', 'info', 'warn'])
  variant?: 'best' | 'rec' | 'info' | 'warn';
}

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
  @IsIn(['px', 'vh'])
  fontUnit?: 'px' | 'vh';

  @IsOptional()
  @IsString()
  color?: string;

  @IsOptional()
  @IsString()
  background?: string;

  @IsOptional()
  @IsNumber()
  opacity?: number;

  @IsOptional()
  @IsNumber()
  fontWeight?: number;

  @IsOptional()
  @IsIn(['left', 'center', 'right'])
  textAlign?: 'left' | 'center' | 'right';

  @IsOptional()
  @IsNumber()
  lineHeight?: number;

  @IsOptional()
  @IsIn(['plain', 'menuLine', 'groupHeader', 'note'])
  textVariant?: 'plain' | 'menuLine' | 'groupHeader' | 'note';

  @IsOptional()
  @IsString()
  textSecondary?: string;

  /** 영문 병기(한글명 옆 작은 회색 텍스트). menuLine/groupHeader에서 사용. */
  @IsOptional()
  @IsString()
  @Length(0, 120)
  textEn?: string;

  /** 이중 가격의 보조가(EXTRA SIZE의 "+1.0" 등). textSecondary(기본가) 뒤에 강조 표시. */
  @IsOptional()
  @IsString()
  @Length(0, 24)
  priceExtra?: string;

  /** menuLine의 가격(textSecondary) 색. 미지정 시 아이템 기본 색 상속. */
  @IsOptional()
  @IsString()
  priceColor?: string;

  /** 인라인 뱃지 배열. 라벨 앞(best/rec) 또는 뒤(info/warn)에 렌더. */
  @IsOptional()
  @IsArray()
  @ArrayMaxSize(4)
  @ValidateNested({ each: true })
  @Type(() => BadgeDto)
  badges?: BadgeDto[];

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

  /** 아이템 배경 모서리 둥글기(px, 디자인 좌표계). 그룹 카드/패널/안내박스용. */
  @IsOptional()
  @IsNumber()
  radius?: number;

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
