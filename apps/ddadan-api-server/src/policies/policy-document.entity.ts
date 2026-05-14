import { Column, Entity, Index } from 'typeorm';
import { BaseEntity } from '../common/base.entity';

export type PolicyKind = 'terms' | 'privacy';

@Entity('policy_documents')
@Index(['kind', 'version'], { unique: true })
export class PolicyDocument extends BaseEntity {
  @Column({ type: 'varchar', length: 16 })
  kind!: PolicyKind;

  @Column({ type: 'varchar', length: 32 })
  version!: string;

  @Column({ type: 'text' })
  content!: string;

  @Column({ type: 'datetime', precision: 6 })
  effectiveAt!: Date;
}
