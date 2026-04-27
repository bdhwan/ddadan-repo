import { Injectable, OnModuleInit } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { Repository } from 'typeorm';
import { CreatePolicyDocumentDto } from './dto/policy.dto';
import { PolicyAcceptance } from './policy-acceptance.entity';
import { PolicyDocument, PolicyKind } from './policy-document.entity';

const TERMS_V1 = `# DDADAN 이용약관 (v1)

본 약관은 DDADAN(이하 "회사")이 제공하는 디지털 사이니지 SaaS 및 하드웨어 서비스(이하 "서비스") 이용에 관한 권리와 의무를 규정합니다.

1. 가입 및 계정
   - 이메일 또는 구글 계정으로 가입할 수 있습니다.
   - 회원은 본인의 정보를 정확히 입력해야 하며, 계정은 양도·대여할 수 없습니다.

2. 서비스 이용
   - 회사는 회원이 직접 등록한 매장, 디바이스, 콘텐츠를 운영할 수 있도록 도구를 제공합니다.
   - 회원은 자신의 콘텐츠가 타인의 권리를 침해하지 않도록 책임을 부담합니다.

3. 회원 탈퇴
   - 회원은 언제든지 탈퇴할 수 있으며, 탈퇴 시 회사가 보유한 회원의 모든 데이터는 소프트 삭제되며 동일 이메일로 즉시 재가입할 수 있습니다.
   - 법령에 따라 보존이 필요한 일부 정보는 일정 기간 보관될 수 있습니다.

4. 면책
   - 회사는 회원이 업로드한 콘텐츠로 인해 발생한 분쟁에 대해 책임지지 않습니다.

5. 약관의 변경
   - 회사는 약관을 변경할 수 있으며, 중요한 변경은 서비스 내에서 사전 공지합니다.
`;

const PRIVACY_V1 = `# DDADAN 개인정보 처리방침 (v1)

회사는 다음과 같이 회원의 개인정보를 수집·이용합니다.

1. 수집 항목
   - 필수: 이메일, 인증 제공자 식별자(Firebase UID), 표시 이름(선택)
   - 자동 수집: 디바이스 하드웨어 식별자, 접속 로그, 모니터 해상도/위치, 콘텐츠 메타데이터

2. 이용 목적
   - 회원 식별 및 서비스 제공
   - 디바이스 등록·관리, 콘텐츠 배포
   - 장애 대응 및 보안

3. 보관 기간
   - 회원 탈퇴 시 즉시 소프트 삭제하며, 이후 법령에서 정한 기간이 지나면 영구 삭제합니다.

4. 제3자 제공
   - 회사는 법령이 요구하는 경우를 제외하고 회원의 개인정보를 제3자에게 제공하지 않습니다.

5. 회원의 권리
   - 회원은 언제든지 자신의 정보를 조회·수정·삭제할 수 있습니다.

6. 문의
   - 개인정보 관련 문의는 서비스 내 문의 채널을 통해 가능합니다.
`;

@Injectable()
export class PoliciesService implements OnModuleInit {
  constructor(
    @InjectRepository(PolicyDocument)
    private readonly docs: Repository<PolicyDocument>,
    @InjectRepository(PolicyAcceptance)
    private readonly acceptances: Repository<PolicyAcceptance>,
  ) {}

  async onModuleInit() {
    await this.ensureSeed('terms', '1', TERMS_V1);
    await this.ensureSeed('privacy', '1', PRIVACY_V1);
  }

  private async ensureSeed(kind: PolicyKind, version: string, content: string) {
    const existing = await this.docs.findOne({ where: { kind, version } });
    if (existing) return;
    const doc = this.docs.create({
      kind,
      version,
      content,
      effectiveAt: new Date(),
    });
    await this.docs.save(doc);
  }

  async getCurrent(kind: PolicyKind): Promise<PolicyDocument | null> {
    const list = await this.docs.find({
      where: { kind },
      order: { effectiveAt: 'DESC' },
      take: 1,
    });
    return list[0] ?? null;
  }

  async listAllCurrent() {
    const [terms, privacy] = await Promise.all([
      this.getCurrent('terms'),
      this.getCurrent('privacy'),
    ]);
    return { terms, privacy };
  }

  async accept(userId: number, documentIds: number[]) {
    const docs = await this.docs.find({
      where: documentIds.map((id) => ({ id })),
    });
    for (const doc of docs) {
      const a = this.acceptances.create({ userId, documentId: doc.id });
      await this.acceptances.save(a);
    }
  }

  async create(dto: CreatePolicyDocumentDto): Promise<PolicyDocument> {
    const doc = this.docs.create({
      kind: dto.kind,
      version: dto.version,
      content: dto.content,
      effectiveAt: dto.effectiveAt ? new Date(dto.effectiveAt) : new Date(),
    });
    return this.docs.save(doc);
  }
}
