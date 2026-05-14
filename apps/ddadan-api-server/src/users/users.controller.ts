import { Controller, Get } from '@nestjs/common';
import { UsersService } from './users.service';

@Controller('users')
export class UsersController {
  constructor(private readonly users: UsersService) {}

  @Get('me')
  async me() {
    const user = await this.users.findById(this.users.getLocalUserId());
    return {
      id: user.id,
      firebaseUid: user.firebaseUid,
      email: user.email,
      name: user.name,
      provider: user.provider,
      createdAt: user.createdAt,
    };
  }
}
