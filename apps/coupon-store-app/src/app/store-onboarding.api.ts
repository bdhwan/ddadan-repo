import { HttpClient } from '@angular/common/http';
import { inject, Injectable } from '@angular/core';
import type {
  OwnerStoreResponseDto,
  SaveOwnerStoreRequestDto,
  SubmitOwnerStoreReviewRequestDto,
  SubmitOwnerStoreReviewResponseDto,
} from '@coupon/contracts';
import { Observable } from 'rxjs';

@Injectable({ providedIn: 'root' })
export class StoreOnboardingApi {
  private readonly http = inject(HttpClient);
  private readonly baseUrl = '/api/coupon/v1/owner/store';

  load(): Observable<OwnerStoreResponseDto> {
    return this.http.get<OwnerStoreResponseDto>(this.baseUrl);
  }

  create(payload: SaveOwnerStoreRequestDto): Observable<OwnerStoreResponseDto> {
    return this.http.post<OwnerStoreResponseDto>(this.baseUrl, payload);
  }

  update(payload: SaveOwnerStoreRequestDto): Observable<OwnerStoreResponseDto> {
    return this.http.patch<OwnerStoreResponseDto>(this.baseUrl, payload);
  }

  submitReview(payload: SubmitOwnerStoreReviewRequestDto): Observable<SubmitOwnerStoreReviewResponseDto> {
    return this.http.post<SubmitOwnerStoreReviewResponseDto>(`${this.baseUrl}/submit-review`, payload);
  }
}
