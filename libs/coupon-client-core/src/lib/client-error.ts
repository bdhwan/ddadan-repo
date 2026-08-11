import { HttpErrorResponse } from "@angular/common/http";
import { Injectable } from "@angular/core";
import type { ErrorResponseDto } from "@coupon/contracts";

export class CouponClientError extends Error {
  constructor(
    message: string,
    readonly code: string,
    readonly retryable: boolean,
    readonly request_id: string | null,
    readonly status: number,
    readonly field_errors: ReadonlyArray<{
      field: string;
      message: string;
    }> = [],
  ) {
    super(message);
    this.name = "CouponClientError";
  }
}

const GENERIC_AUTH_MESSAGE = "이메일 또는 비밀번호를 확인해 주세요.";

@Injectable({ providedIn: "root" })
export class CouponErrorMapper {
  from(error: unknown): CouponClientError {
    if (error instanceof CouponClientError) {
      return error;
    }

    if (error instanceof HttpErrorResponse) {
      const body = error.error as Partial<ErrorResponseDto> | null;
      const detail = body?.error;
      const code = detail?.code ?? `HTTP_${error.status}`;
      const retryable = detail?.retryable ?? false;
      const message = this.userMessage(
        error.status,
        code,
        retryable,
        detail?.message,
      );
      return new CouponClientError(
        message,
        code,
        retryable,
        detail?.request_id ?? null,
        error.status,
        detail?.field_errors ?? [],
      );
    }

    const firebaseCode = this.firebaseErrorCode(error);
    if (firebaseCode) {
      return new CouponClientError(
        this.firebaseMessage(firebaseCode),
        firebaseCode,
        false,
        null,
        0,
      );
    }

    return new CouponClientError(
      "요청을 처리하지 못했습니다. 잠시 후 다시 시도해 주세요.",
      "UNKNOWN",
      false,
      null,
      0,
    );
  }

  private userMessage(
    status: number,
    code: string,
    retryable: boolean,
    serverMessage?: string,
  ): string {
    if (status === 401 || code.startsWith("AUTH_")) {
      return GENERIC_AUTH_MESSAGE;
    }
    if (status === 429) {
      return retryable
        ? "요청이 많습니다. 잠시 후 다시 시도해 주세요."
        : "요청 한도를 초과했습니다. 안내된 시간 후에 다시 이용해 주세요.";
    }
    if (status === 503) {
      return retryable
        ? "서비스가 잠시 불안정합니다. 안전하게 다시 시도할 수 있습니다."
        : "처리 상태를 확인하기 전에 같은 요청을 반복하지 마세요.";
    }
    return serverMessage ?? "요청을 처리하지 못했습니다.";
  }

  private firebaseErrorCode(error: unknown): string | null {
    if (typeof error === "object" && error !== null && "code" in error) {
      const code = (error as { code?: unknown }).code;
      return typeof code === "string" && code.startsWith("auth/") ? code : null;
    }
    return null;
  }

  private firebaseMessage(code: string): string {
    if (
      [
        "auth/invalid-credential",
        "auth/user-not-found",
        "auth/wrong-password",
        "auth/invalid-email",
      ].includes(code)
    ) {
      return GENERIC_AUTH_MESSAGE;
    }
    if (code === "auth/too-many-requests") {
      return "시도 횟수가 많습니다. 잠시 후 다시 시도해 주세요.";
    }
    return "로그인을 완료하지 못했습니다. 다시 시도해 주세요.";
  }
}
