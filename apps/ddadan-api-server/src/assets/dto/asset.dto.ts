import { IsIn, IsInt, IsOptional, IsString, Length } from 'class-validator';

export class CreateTextAssetDto {
  @IsString()
  @Length(1, 200)
  originalName!: string;

  @IsString()
  textContent!: string;

  @IsOptional()
  @IsInt()
  storeId?: number;
}

export class UpdateAssetDto {
  @IsOptional()
  @IsString()
  @Length(1, 200)
  originalName?: string;

  @IsOptional()
  @IsString()
  textContent?: string;
}

export class AssetQueryDto {
  @IsOptional()
  @IsIn(['image', 'video', 'text'])
  type?: 'image' | 'video' | 'text';

  @IsOptional()
  @IsInt()
  storeId?: number;
}
