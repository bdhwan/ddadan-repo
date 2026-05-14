import { Body, Controller, Get, Post } from '@nestjs/common';
import { UsersService } from '../users/users.service';
import { AcceptPoliciesDto } from './dto/policy.dto';
import { PoliciesService } from './policies.service';

@Controller('policies')
export class PoliciesController {
  constructor(
    private readonly policies: PoliciesService,
    private readonly users: UsersService,
  ) {}

  @Get('current')
  current() {
    return this.policies.listAllCurrent();
  }

  @Post('accept')
  async accept(@Body() dto: AcceptPoliciesDto) {
    await this.policies.accept(this.users.getLocalUserId(), dto.documentIds);
    return { ok: true };
  }
}
