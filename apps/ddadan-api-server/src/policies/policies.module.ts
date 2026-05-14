import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { UsersModule } from '../users/users.module';
import { PoliciesController } from './policies.controller';
import { PoliciesService } from './policies.service';
import { PolicyAcceptance } from './policy-acceptance.entity';
import { PolicyDocument } from './policy-document.entity';

@Module({
  imports: [
    TypeOrmModule.forFeature([PolicyDocument, PolicyAcceptance]),
    UsersModule,
  ],
  providers: [PoliciesService],
  controllers: [PoliciesController],
  exports: [PoliciesService],
})
export class PoliciesModule {}
