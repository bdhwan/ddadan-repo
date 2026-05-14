import { Column, Entity, Index, JoinColumn, ManyToOne } from 'typeorm';
import { BaseEntity } from '../common/base.entity';
import { User } from '../users/user.entity';

@Entity('stores')
@Index(['ownerUserId'])
export class Store extends BaseEntity {
  @Column({ type: 'integer' })
  ownerUserId!: number;

  @ManyToOne(() => User, { onDelete: 'CASCADE' })
  @JoinColumn({ name: 'ownerUserId' })
  owner?: User;

  @Column({ type: 'varchar', length: 200 })
  name!: string;

  @Column({ type: 'varchar', length: 64, nullable: true })
  businessType!: string | null;

  @Column({ type: 'varchar', length: 64, default: 'Asia/Seoul' })
  timezone!: string;
}
