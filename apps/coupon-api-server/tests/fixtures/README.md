# 테스트 전용 키

여기 있는 RSA 키는 **테스트에서만** 쓰는, 이 저장소를 위해 그 자리에서 만든 일회용
키다. 어떤 실제 서비스의 자격증명도 아니고, 어디에도 등록되어 있지 않다.

| 파일 | 무엇 |
|---|---|
| `kakao-signing-key-1.pem` / `.jwk.json` | contract mock 이 카카오인 척 `id_token` 을 서명할 때 쓰는 키. `kid=mock-key-1` |
| `kakao-signing-key-2.pem` / `.jwk.json` | 같은 용도의 두 번째 키. §9.2-4 의 **JWKS `kid` 회전**을 재현하려면 두 개가 필요하다 |
| `firebase-service-account-key.pem` | Firebase Custom Token 서명 경로(§9.2-7)를 실제로 밟아 보기 위한 RS256 키 |

`firebase-service-account-key.pem` 은 **가짜 Firebase 서비스 계정이 아니다.** 서비스
계정 JSON 도, 프로젝트에 연결된 키도 아니다. 그저 "RS256 으로 서명해서 다시 검증하면
같은 클레임이 나오는가"를 확인하기 위한 키다. 실제 Firebase 서비스 계정 키는 아직
없으며, 없을 때 서버가 명확한 오류로 거절하는지도 같은 스위트에서 검증한다
(`kakao_sign_in_fails_clearly_when_no_firebase_service_account_is_configured`).

운영 설정에는 절대 쓰지 말 것.
