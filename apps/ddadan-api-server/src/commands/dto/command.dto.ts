import { IsIn, IsOptional, IsString, Length } from 'class-validator';
import type { CommandType } from '../device-command.entity';

const TYPES: CommandType[] = [
  'reboot',
  'screenOn',
  'screenOff',
  'updateApp',
  'shell',
];

export class CreateCommandDto {
  @IsString()
  @IsIn(TYPES)
  type!: CommandType;

  @IsOptional()
  @IsString()
  @Length(0, 4000)
  payload?: string;
}

export class AckCommandDto {
  @IsString()
  @IsIn(['done', 'failed'])
  status!: 'done' | 'failed';

  @IsOptional()
  @IsString()
  @Length(0, 4000)
  result?: string;
}
