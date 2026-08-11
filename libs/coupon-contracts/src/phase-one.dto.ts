/** RFC 3339 timestamp, serialized as a string at the HTTP boundary. */
export type Rfc3339Timestamp = string;
export type Uuid = string;
export type Currency = "KRW";

export interface FieldErrorDto {
  field: string;
  message: string;
  code?: string;
}

export interface ErrorDetailDto {
  code: string;
  message: string;
  field_errors: FieldErrorDto[];
  retryable: boolean;
  request_id: string;
}

export interface ErrorResponseDto {
  error: ErrorDetailDto;
}

export interface CursorPageDto<T> {
  items: T[];
  next_cursor: string | null;
  request_id: string;
}

export interface MoneyDto {
  amount: number;
  currency: Currency;
}

export interface UserBootstrapRequestDto {
  display_name: string;
  locale?: string;
  timezone?: string;
}

export interface UserBootstrapResponseDto {
  user: MeProfileDto;
  created: boolean;
  request_id: string;
  transaction_id: string;
}

export interface MeProfileDto {
  id: Uuid;
  email: string | null;
  display_name: string;
  email_verified: boolean;
  status: "ACTIVE" | "SUSPENDED" | "WITHDRAWAL_PENDING" | "WITHDRAWN";
  created_at: Rfc3339Timestamp;
  updated_at: Rfc3339Timestamp;
  version: number;
}

export interface UpdateMeRequestDto {
  display_name?: string;
  version: number;
}

export type UserRole = "CONSUMER" | "STORE_OWNER" | "SYSTEM_ADMIN";

export interface MeRolesResponseDto {
  roles: Array<{
    role: UserRole;
    store_id: Uuid | null;
    status: "ACTIVE" | "PENDING" | "SUSPENDED";
  }>;
  request_id: string;
}

export type ConsentPurpose =
  | "TERMS_OF_SERVICE"
  | "PRIVACY_POLICY"
  | "MARKETING_PUSH"
  | "MARKETING_KAKAO";

export interface ConsentDto {
  purpose: ConsentPurpose;
  granted: boolean;
  version: string;
  decided_at: Rfc3339Timestamp;
}

export interface MeConsentsResponseDto {
  consents: ConsentDto[];
  request_id: string;
}

export interface UpdateMeConsentsRequestDto {
  consents: Array<Pick<ConsentDto, "purpose" | "granted" | "version">>;
}

export type StoreReviewStatus =
  | "DRAFT"
  | "IN_REVIEW"
  | "CHANGES_REQUESTED"
  | "APPROVED"
  | "REJECTED"
  | "SUSPENDED";

export interface BusinessHoursDto {
  day_of_week: 0 | 1 | 2 | 3 | 4 | 5 | 6;
  opens_at: string | null;
  closes_at: string | null;
  closed: boolean;
}

export interface OwnerStoreDto {
  id: Uuid;
  slug: string | null;
  name: string;
  description: string | null;
  business_registration_number_masked: string | null;
  representative_name_masked: string | null;
  address: string | null;
  timezone: "Asia/Seoul";
  business_hours: BusinessHoursDto[];
  review_status: StoreReviewStatus;
  review_reason: string | null;
  submitted_at: Rfc3339Timestamp | null;
  created_at: Rfc3339Timestamp;
  updated_at: Rfc3339Timestamp;
  version: number;
}

export interface SaveOwnerStoreRequestDto {
  name?: string;
  description?: string | null;
  business_registration_number?: string;
  representative_name?: string;
  address?: string | null;
  timezone?: "Asia/Seoul";
  business_hours?: BusinessHoursDto[];
  accepted_terms_version?: string;
  version?: number;
}

export interface OwnerStoreResponseDto {
  store: OwnerStoreDto;
  request_id: string;
  transaction_id?: string;
}

export interface SubmitOwnerStoreReviewRequestDto {
  version: number;
}

export interface SubmitOwnerStoreReviewResponseDto {
  store: OwnerStoreDto;
  request_id: string;
  transaction_id: string;
}
