import { Transform } from 'class-transformer';
import { ArrayMaxSize, ArrayMinSize, IsArray, IsInt, Max, Min, IsOptional, ValidateIf } from 'class-validator';

export class UpdateMonitorPositionDto {
  @IsInt()
  positionX!: number;

  @IsInt()
  positionY!: number;
}

export class AssignScreenDto {
  @IsOptional()
  @Transform(({ value }) =>
    value === null || value === undefined || value === '' ? null : Number(value),
  )
  @ValidateIf((_, v) => v !== null && v !== undefined)
  @IsInt()
  screenId?: number | null;
}

export class SetMonitorRotationDto {
  /** Ordered screen ids (appearance order in admin). Empty = clear rotation & unassign. */
  @IsArray()
  @ArrayMinSize(0)
  @ArrayMaxSize(32)
  @IsInt({ each: true })
  screenIds!: number[];

  @IsInt()
  @Min(2_000)
  @Max(3600_000)
  intervalMs!: number;

  @IsInt()
  @Min(200)
  @Max(10_000)
  fadeMs!: number;
}
