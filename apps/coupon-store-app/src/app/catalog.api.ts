import { HttpClient } from '@angular/common/http';
import { inject, Injectable } from '@angular/core';
import type { CatalogItemDto, CatalogItemListResponseDto, SaveCatalogItemRequestDto } from '@coupon/contracts';
import { Observable } from 'rxjs';

@Injectable({providedIn:'root'})
export class CatalogApi{
  private readonly http=inject(HttpClient);private readonly base='/api/coupon/v1/owner/catalog/items';
  list():Observable<CatalogItemListResponseDto>{return this.http.get<CatalogItemListResponseDto>(this.base);}
  create(payload:SaveCatalogItemRequestDto):Observable<CatalogItemDto>{return this.http.post<CatalogItemDto>(this.base,payload);}
  update(id:string,payload:SaveCatalogItemRequestDto):Observable<CatalogItemDto>{return this.http.patch<CatalogItemDto>(`${this.base}/${id}`,payload);}
}
