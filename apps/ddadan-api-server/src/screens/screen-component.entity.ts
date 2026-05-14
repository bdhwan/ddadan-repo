import { Column, Entity, Index, JoinColumn, ManyToOne } from 'typeorm';
import { BaseEntity } from '../common/base.entity';
import { User } from '../users/user.entity';
import { ScreenLayoutItem } from './screen.entity';

@Entity('screen_components')
@Index(['ownerUserId'])
export class ScreenComponent extends BaseEntity {
  @Column({ type: 'integer' })
  ownerUserId!: number;

  @ManyToOne(() => User, { onDelete: 'CASCADE' })
  @JoinColumn({ name: 'ownerUserId' })
  owner?: User;

  @Column({ type: 'varchar', length: 200 })
  name!: string;

  @Column({ type: 'varchar', length: 32 })
  kind!: 'image' | 'video' | 'text' | 'group';

  @Column({ type: 'json' })
  payload!: ScreenLayoutItem | ScreenLayoutItem[];
}
