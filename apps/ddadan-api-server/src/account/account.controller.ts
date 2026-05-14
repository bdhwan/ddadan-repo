import { Controller, Delete, HttpCode } from '@nestjs/common';
import { UsersService } from '../users/users.service';
import { AccountService } from './account.service';

@Controller('account')
export class AccountController {
  constructor(
    private readonly account: AccountService,
    private readonly users: UsersService,
  ) {}

  @Delete()
  @HttpCode(204)
  async withdraw() {
    await this.account.withdraw(this.users.getLocalUserId());
  }
}
