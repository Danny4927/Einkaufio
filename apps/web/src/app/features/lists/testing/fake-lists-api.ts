import { ApiError } from '../../../core/api/api-error';
import {
  ItemDetails,
  ItemMutationResult,
  ListVersionResult,
  ReorderResult,
  ShoppingItem,
  ShoppingListSummary,
  SyncResponse,
} from '../../../core/api/api.models';

interface StoredItem {
  item: ShoppingItem;
  changedAt: number;
}

/**
 * In-memory stand-in for the list routes of ListsApiService with the server's concurrency
 * rules: every mutation bumps the list version by one, expected versions are checked (409),
 * and `sync(since)` returns the changes after that version.
 */
export class FakeListsApi {
  version = 1;
  list: ShoppingListSummary = { id: 'l1', name: 'Wocheneinkauf', type: 'PERSONAL', version: 1 };
  private readonly items = new Map<string, StoredItem>();
  private readonly removed = new Map<string, number>();
  private nextId = 1;
  private gate: Promise<void> | null = null;
  private failure: ApiError | null = null;
  missing = false;
  readonly calls: string[] = [];

  /** Seeds items as another client would have created them. */
  seed(...names: string[]): ShoppingItem[] {
    return names.map((name) => this.otherClientAdds(name));
  }

  otherClientAdds(name: string): ShoppingItem {
    this.version += 1;
    const item: ShoppingItem = { id: `i${this.nextId++}`, name, checked: false, position: this.items.size, itemVersion: 1 };
    this.items.set(item.id, { item, changedAt: this.version });
    return item;
  }

  otherClientChecks(itemId: string): void {
    const stored = this.require(itemId);
    this.version += 1;
    this.store({ ...stored.item, checked: !stored.item.checked, itemVersion: stored.item.itemVersion + 1 });
  }

  otherClientDeletes(itemId: string): void {
    this.require(itemId);
    this.version += 1;
    this.items.delete(itemId);
    this.removed.set(itemId, this.version);
  }

  serverItems(): ShoppingItem[] {
    return [...this.items.values()].map((stored) => stored.item).sort((a, b) => a.position - b.position);
  }

  /** Holds every response until the returned function is called. */
  hold(): () => void {
    let release!: () => void;
    this.gate = new Promise((resolve) => (release = resolve));
    return () => {
      this.gate = null;
      release();
    };
  }

  failNext(kind: ApiError['kind'], status: number): void {
    this.failure = new ApiError(kind, status);
  }

  async sync(listId: string, since?: number): Promise<SyncResponse> {
    await this.enter(`sync ${listId}${since === undefined ? '' : ` since=${since}`}`);
    const list = { ...this.list, version: this.version };
    if (since === undefined) {
      return { kind: 'snapshot', version: this.version, list, items: this.serverItems() };
    }
    const changed = [...this.items.values()].filter((stored) => stored.changedAt > since).map((stored) => stored.item);
    const removed = [...this.removed.entries()].filter(([, at]) => at > since).map(([id]) => id);
    return { kind: 'changes', version: this.version, since, list, changed, removed };
  }

  async addItem(listId: string, expectedListVersion: number, details: ItemDetails): Promise<ItemMutationResult> {
    await this.enter(`add ${details.name}`);
    this.expectList(expectedListVersion);
    this.version += 1;
    const item: ShoppingItem = { id: `i${this.nextId++}`, ...details, checked: false, position: this.items.size, itemVersion: 1 };
    this.items.set(item.id, { item, changedAt: this.version });
    return { item, listVersion: this.version };
  }

  async setItemChecked(listId: string, itemId: string, expectedVersion: number, checked: boolean): Promise<ItemMutationResult> {
    await this.enter(`check ${itemId}=${checked}`);
    const stored = this.expectItem(itemId, expectedVersion);
    this.version += 1;
    const item = this.store({ ...stored.item, checked, itemVersion: stored.item.itemVersion + 1 });
    return { item, listVersion: this.version };
  }

  async updateItem(listId: string, itemId: string, expectedVersion: number, details: ItemDetails): Promise<ItemMutationResult> {
    await this.enter(`update ${itemId}`);
    const stored = this.expectItem(itemId, expectedVersion);
    this.version += 1;
    const { id, checked, position, itemVersion } = stored.item;
    const item = this.store({ id, checked, position, itemVersion: itemVersion + 1, ...details });
    return { item, listVersion: this.version };
  }

  async deleteItem(listId: string, itemId: string, expectedVersion: number): Promise<ListVersionResult> {
    await this.enter(`delete ${itemId}`);
    this.expectItem(itemId, expectedVersion);
    this.version += 1;
    this.items.delete(itemId);
    this.removed.set(itemId, this.version);
    return { listVersion: this.version };
  }

  async reorderItems(listId: string, expectedListVersion: number, itemIds: readonly string[]): Promise<ReorderResult> {
    await this.enter(`reorder ${itemIds.join(',')}`);
    this.expectList(expectedListVersion);
    if (itemIds.length !== this.items.size || itemIds.some((id) => !this.items.has(id))) {
      throw new ApiError('invalid', 400);
    }
    this.version += 1;
    itemIds.forEach((id, position) => {
      const stored = this.require(id);
      if (stored.item.position !== position) {
        this.store({ ...stored.item, position, itemVersion: stored.item.itemVersion + 1 });
      }
    });
    return { items: this.serverItems(), listVersion: this.version };
  }

  private async enter(call: string): Promise<void> {
    this.calls.push(call);
    if (this.gate !== null) {
      await this.gate;
    }
    if (this.missing) {
      throw new ApiError('not-found', 404);
    }
    const failure = this.failure;
    if (failure !== null) {
      this.failure = null;
      throw failure;
    }
  }

  private expectList(expected: number): void {
    if (expected !== this.version) {
      throw new ApiError('conflict', 409);
    }
  }

  private expectItem(itemId: string, expected: number): StoredItem {
    const stored = this.items.get(itemId);
    if (stored === undefined) {
      throw new ApiError('not-found', 404);
    }
    if (stored.item.itemVersion !== expected) {
      throw new ApiError('conflict', 409);
    }
    return stored;
  }

  private require(itemId: string): StoredItem {
    const stored = this.items.get(itemId);
    if (stored === undefined) {
      throw new Error(`unknown item ${itemId}`);
    }
    return stored;
  }

  private store(item: ShoppingItem): ShoppingItem {
    this.items.set(item.id, { item, changedAt: this.version });
    return item;
  }
}
