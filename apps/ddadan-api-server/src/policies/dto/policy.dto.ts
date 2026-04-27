import { IsArray, IsInt, IsOptional, IsString } from 'class-validator';

export class AcceptPoliciesDto {
  @IsArray()
  @IsInt({ each: true })
  documentIds!: number[];
}

export class CreatePolicyDocumentDto {
  @IsString()
  kind!: 'terms' | 'privacy';

  @IsString()
  version!: string;

  @IsString()
  content!: string;

  @IsOptional()
  @IsString()
  effectiveAt?: string;
}
