import { Column, Entity, Index } from 'typeorm';
import { BaseEntity } from '../common/base.entity';

@Entity('users')
@Index(['email'])
export class User extends BaseEntity {
  @Column({ type: 'varchar', length: 128, unique: true })
  firebaseUid!: string;

  @Column({ type: 'varchar', length: 320, nullable: true })
  email!: string | null;

  @Column({ type: 'varchar', length: 200, nullable: true })
  name!: string | null;

  @Column({ type: 'varchar', length: 64 })
  provider!: string;
}
