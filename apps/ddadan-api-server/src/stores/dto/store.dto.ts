import { IsOptional, IsString, Length } from 'class-validator';

export class CreateStoreDto {
  @IsString()
  @Length(1, 200)
  name!: string;

  @IsOptional()
  @IsString()
  @Length(0, 64)
  businessType?: string;

  @IsOptional()
  @IsString()
  @Length(0, 64)
  timezone?: string;
}

export class UpdateStoreDto {
  @IsOptional()
  @IsString()
  @Length(1, 200)
  name?: string;

  @IsOptional()
  @IsString()
  @Length(0, 64)
  businessType?: string;

  @IsOptional()
  @IsString()
  @Length(0, 64)
  timezone?: string;
}
