import { Body, Controller, Get, Post } from '@nestjs/common';
import { CurrentUser } from '../auth/current-user.decorator';
import type { AuthContext } from '../auth/firebase-auth.guard';
import { Public } from '../auth/public.decorator';
import { AcceptPoliciesDto } from './dto/policy.dto';
import { PoliciesService } from './policies.service';

@Controller('policies')
export class PoliciesController {
  constructor(private readonly policies: PoliciesService) {}

  @Public()
  @Get('current')
  current() {
    return this.policies.listAllCurrent();
  }

  @Post('accept')
  async accept(
    @CurrentUser() auth: AuthContext,
    @Body() dto: AcceptPoliciesDto,
  ) {
    await this.policies.accept(auth.userId, dto.documentIds);
    return { ok: true };
  }
}
