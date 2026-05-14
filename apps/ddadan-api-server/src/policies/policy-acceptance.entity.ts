import {
  Column,
  CreateDateColumn,
  Entity,
  Index,
  JoinColumn,
  ManyToOne,
  PrimaryGeneratedColumn,
} from 'typeorm';
import { User } from '../users/user.entity';
import { PolicyDocument } from './policy-document.entity';

@Entity('policy_acceptances')
@Index(['userId'])
export class PolicyAcceptance {
  @PrimaryGeneratedColumn({ type: 'integer' })
  id!: number;

  @Column({ type: 'integer' })
  userId!: number;

  @ManyToOne(() => User, { onDelete: 'CASCADE' })
  @JoinColumn({ name: 'userId' })
  user?: User;

  @Column({ type: 'integer' })
  documentId!: number;

  @ManyToOne(() => PolicyDocument, { onDelete: 'CASCADE' })
  @JoinColumn({ name: 'documentId' })
  document?: PolicyDocument;

  @CreateDateColumn({ type: 'datetime', precision: 6 })
  acceptedAt!: Date;
}
