import { IsInt, IsOptional } from 'class-validator';

export class UpdateMonitorPositionDto {
  @IsInt()
  positionX!: number;

  @IsInt()
  positionY!: number;
}

export class AssignScreenDto {
  @IsOptional()
  @IsInt()
  screenId!: number | null;
}
