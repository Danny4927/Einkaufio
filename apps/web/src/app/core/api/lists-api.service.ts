import { HttpClient, HttpParams } from '@angular/common/http';
import { inject, Injectable } from '@angular/core';
import { firstValueFrom, Observable } from 'rxjs';
import { RUNTIME_CONFIG } from '../config/runtime-config';
import { toApiError } from './api-error';
import {
  ItemDetails,
  ItemMutationResult,
  ListPage,
  ListVersionResult,
  ReorderResult,
  ShoppingListSummary,
  SyncResponse,
} from './api.models';

/**
 * Typed client for list and item routes. Every method rejects with an {@link ApiError}.
 * The actor is never sent: the API derives it from the verified access token.
 */
@Injectable({ providedIn: 'root' })
export class ListsApiService {
  private readonly http = inject(HttpClient);
  private readonly baseUrl = `${inject(RUNTIME_CONFIG).apiBaseUrl}/api`;

  listLists(cursor?: string): Promise<ListPage> {
    const params = cursor === undefined ? undefined : new HttpParams().set('cursor', cursor);
    return this.send(this.http.get<ListPage>(`${this.baseUrl}/lists`, { params }));
  }

  createList(name: string): Promise<ShoppingListSummary> {
    return this.send(this.http.post<ShoppingListSummary>(`${this.baseUrl}/lists`, { name }));
  }

  /** Full snapshot without `since`; otherwise the changes after that list version. */
  sync(listId: string, since?: number): Promise<SyncResponse> {
    const params = since === undefined ? undefined : new HttpParams().set('since', since);
    return this.send(this.http.get<SyncResponse>(`${this.listUrl(listId)}/sync`, { params }));
  }

  addItem(listId: string, expectedListVersion: number, details: ItemDetails): Promise<ItemMutationResult> {
    return this.send(
      this.http.post<ItemMutationResult>(`${this.listUrl(listId)}/items`, { expectedListVersion, ...details }),
    );
  }

  setItemChecked(listId: string, itemId: string, expectedVersion: number, checked: boolean): Promise<ItemMutationResult> {
    return this.send(this.http.patch<ItemMutationResult>(this.itemUrl(listId, itemId), { expectedVersion, checked }));
  }

  updateItem(listId: string, itemId: string, expectedVersion: number, details: ItemDetails): Promise<ItemMutationResult> {
    return this.send(this.http.patch<ItemMutationResult>(this.itemUrl(listId, itemId), { expectedVersion, ...details }));
  }

  deleteItem(listId: string, itemId: string, expectedVersion: number): Promise<ListVersionResult> {
    const params = new HttpParams().set('expectedVersion', expectedVersion);
    return this.send(this.http.delete<ListVersionResult>(this.itemUrl(listId, itemId), { params }));
  }

  /** `itemIds` must be the complete new order of the list. */
  reorderItems(listId: string, expectedListVersion: number, itemIds: readonly string[]): Promise<ReorderResult> {
    return this.send(
      this.http.put<ReorderResult>(`${this.listUrl(listId)}/item-order`, { expectedListVersion, itemIds }),
    );
  }

  private listUrl(listId: string): string {
    return `${this.baseUrl}/lists/${encodeURIComponent(listId)}`;
  }

  private itemUrl(listId: string, itemId: string): string {
    return `${this.listUrl(listId)}/items/${encodeURIComponent(itemId)}`;
  }

  private async send<T>(request: Observable<T>): Promise<T> {
    try {
      return await firstValueFrom(request);
    } catch (error) {
      throw toApiError(error);
    }
  }
}
