import { Controller, Delete, HttpCode } from '@nestjs/common';
import { CurrentUser } from '../auth/current-user.decorator';
import type { AuthContext } from '../auth/firebase-auth.guard';
import { AccountService } from './account.service';

@Controller('account')
export class AccountController {
  constructor(private readonly account: AccountService) {}

  @Delete()
  @HttpCode(204)
  async withdraw(@CurrentUser() auth: AuthContext) {
    await this.account.withdraw(auth.userId, auth.firebaseUid);
  }
}
