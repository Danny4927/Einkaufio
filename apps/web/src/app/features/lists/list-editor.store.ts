import { computed, inject, Injectable, signal } from '@angular/core';
import { ApiError, toApiError } from '../../core/api/api-error';
import { ItemDetails, ShoppingItem, ShoppingListSummary, SyncResponse } from '../../core/api/api.models';
import { ListsApiService } from '../../core/api/lists-api.service';
import {
  applyChanges,
  canMove,
  isLocalItem,
  LOCAL_ITEM_PREFIX,
  moveItem,
  nextPosition,
  removeItem,
  replaceItem,
  sortByPosition,
  upsertItem,
} from './item-ops';

export type EditorState = 'loading' | 'ready' | 'not-found' | 'error';

export interface EditorNotice {
  readonly kind: 'conflict' | 'error';
  readonly text: string;
}

type ItemsUpdate = (items: readonly ShoppingItem[]) => readonly ShoppingItem[];

/** What a confirmed mutation changes in the server-confirmed state. */
interface Commit {
  readonly listVersion: number;
  readonly apply: ItemsUpdate;
}

interface PendingOperation {
  /** Optimistic effect, re-applied on top of every new confirmed state until settled. */
  readonly project: ItemsUpdate;
  readonly send: () => Promise<Commit>;
}

export const CONFLICT_NOTICE =
  'Die Liste wurde zwischenzeitlich geändert. Der aktuelle Stand ist geladen – bitte prüfe deine letzte Änderung.';

/**
 * State of the one list that is open in the editor.
 *
 * The UI shows `confirmed` (last server state) with all pending operations projected on top,
 * so polling never overwrites optimistic changes. Requests run strictly one after another;
 * each mutation reads the expected versions from the confirmed state when it is sent.
 * A 409 (or a vanished item) drops all pending operations and reloads a fresh snapshot.
 */
@Injectable()
export class ListEditorStore {
  private readonly api = inject(ListsApiService);
  private readonly confirmed = signal<readonly ShoppingItem[]>([]);
  private readonly pending = signal<readonly PendingOperation[]>([]);
  private listId: string | null = null;
  private confirmedVersion = 0;
  private generation = 0;
  private localIds = 0;
  private syncQueued = false;
  private queue: Promise<void> = Promise.resolve();

  readonly list = signal<ShoppingListSummary | null>(null);
  readonly state = signal<EditorState>('loading');
  readonly notice = signal<EditorNotice | null>(null);
  readonly syncProblem = signal(false);
  readonly items = computed(() =>
    sortByPosition(this.pending().reduce<readonly ShoppingItem[]>((items, op) => op.project(items), this.confirmed())),
  );
  readonly checkedCount = computed(() => this.items().filter((item) => item.checked).length);
  readonly saving = computed(() => this.pending().length > 0);

  /** Loads a full snapshot of `listId`; anything still running for a previous list is ignored. */
  open(listId: string): Promise<void> {
    this.generation += 1;
    this.listId = listId;
    this.confirmedVersion = 0;
    this.syncQueued = false;
    this.confirmed.set([]);
    this.pending.set([]);
    this.list.set(null);
    this.notice.set(null);
    this.syncProblem.set(false);
    this.state.set('loading');
    return this.enqueue(() => this.loadSnapshot());
  }

  /** Poll for changes; skipped while not ready, while mutations are pending or a sync is queued. */
  refresh(): Promise<void> {
    if (this.state() !== 'ready' || this.pending().length > 0 || this.syncQueued) {
      return Promise.resolve();
    }
    this.syncQueued = true;
    const generation = this.generation;
    return this.enqueue(async () => {
      try {
        await this.syncChanges();
      } finally {
        if (generation === this.generation) {
          this.syncQueued = false;
        }
      }
    });
  }

  /** Resolves once every queued request has settled (used by tests and the page). */
  whenIdle(): Promise<void> {
    return this.queue;
  }

  dismissNotice(): void {
    this.notice.set(null);
  }

  addItem(details: ItemDetails): void {
    this.localIds += 1;
    const localId = `${LOCAL_ITEM_PREFIX}${this.localIds}`;
    this.mutate({
      project: (items) => [...items, { ...details, id: localId, checked: false, position: nextPosition(items), itemVersion: 0 }],
      send: async () => {
        const result = await this.api.addItem(this.requireListId(), this.confirmedVersion, details);
        return { listVersion: result.listVersion, apply: (items) => upsertItem(items, result.item) };
      },
    });
  }

  toggleItem(itemId: string): void {
    const current = this.editableItem(itemId);
    if (current === null) {
      return;
    }
    const checked = !current.checked;
    this.mutate({
      project: (items) => replaceItem(items, itemId, (item) => ({ ...item, checked })),
      send: async () => {
        const target = this.confirmedItem(itemId);
        const result = await this.api.setItemChecked(this.requireListId(), itemId, target.itemVersion, checked);
        return { listVersion: result.listVersion, apply: (items) => upsertItem(items, result.item) };
      },
    });
  }

  updateItem(itemId: string, details: ItemDetails): void {
    if (this.editableItem(itemId) === null) {
      return;
    }
    this.mutate({
      project: (items) =>
        replaceItem(items, itemId, (item) => ({
          id: item.id,
          checked: item.checked,
          position: item.position,
          itemVersion: item.itemVersion,
          ...details,
        })),
      send: async () => {
        const target = this.confirmedItem(itemId);
        const result = await this.api.updateItem(this.requireListId(), itemId, target.itemVersion, details);
        return { listVersion: result.listVersion, apply: (items) => upsertItem(items, result.item) };
      },
    });
  }

  deleteItem(itemId: string): void {
    if (this.editableItem(itemId) === null) {
      return;
    }
    this.mutate({
      project: (items) => removeItem(items, itemId),
      send: async () => {
        const target = this.confirmedItem(itemId);
        const result = await this.api.deleteItem(this.requireListId(), itemId, target.itemVersion);
        return { listVersion: result.listVersion, apply: (items) => removeItem(items, itemId) };
      },
    });
  }

  canMove(itemId: string, delta: -1 | 1): boolean {
    return canMove(this.items(), itemId, delta);
  }

  moveItem(itemId: string, delta: -1 | 1): void {
    if (!this.canMove(itemId, delta)) {
      return;
    }
    this.mutate({
      project: (items) => moveItem(items, itemId, delta),
      send: async () => {
        this.confirmedItem(itemId);
        const order = moveItem(this.confirmed(), itemId, delta).map((item) => item.id);
        const result = await this.api.reorderItems(this.requireListId(), this.confirmedVersion, order);
        return { listVersion: result.listVersion, apply: () => result.items };
      },
    });
  }

  private mutate(operation: PendingOperation): void {
    const generation = this.generation;
    this.notice.set(null);
    this.pending.update((operations) => [...operations, operation]);
    void this.enqueue(async () => {
      if (generation !== this.generation || !this.pending().includes(operation)) {
        return;
      }
      try {
        const commit = await operation.send();
        if (generation !== this.generation) {
          return;
        }
        // Commit and settle in one synchronous step so the item never shows twice.
        this.confirmed.update(commit.apply);
        this.pending.update((operations) => operations.filter((candidate) => candidate !== operation));
        if (!this.advanceVersion(commit.listVersion)) {
          await this.syncChanges();
        }
      } catch (error) {
        if (generation === this.generation) {
          await this.handleMutationFailure(toApiError(error), operation);
        }
      }
    });
  }

  /**
   * Accepts the new list version when it is exactly ours (+1) or unchanged (no-op update).
   * A larger jump means someone else changed the list too: keep the old version so the
   * following sync fetches every change since then.
   */
  private advanceVersion(listVersion: number): boolean {
    if (listVersion > this.confirmedVersion + 1) {
      return false;
    }
    this.confirmedVersion = Math.max(this.confirmedVersion, listVersion);
    return true;
  }

  private async handleMutationFailure(error: ApiError, operation: PendingOperation): Promise<void> {
    if (error.kind === 'conflict' || error.kind === 'not-found') {
      this.pending.set([]);
      this.notice.set({ kind: 'conflict', text: CONFLICT_NOTICE });
      await this.loadSnapshot();
      return;
    }
    if (error.kind === 'unauthorized') {
      // The interceptor already ended the session and navigates to sign-in.
      this.pending.set([]);
      return;
    }
    this.pending.update((operations) => operations.filter((candidate) => candidate !== operation));
    this.notice.set({ kind: 'error', text: failureText(error) });
  }

  private async loadSnapshot(): Promise<void> {
    const generation = this.generation;
    try {
      const response = await this.api.sync(this.requireListId());
      if (generation === this.generation) {
        this.applySync(response);
        this.syncProblem.set(false);
        this.state.set('ready');
      }
    } catch (error) {
      if (generation === this.generation) {
        const kind = toApiError(error).kind;
        this.state.set(kind === 'not-found' ? 'not-found' : 'error');
      }
    }
  }

  private async syncChanges(): Promise<void> {
    const generation = this.generation;
    try {
      const response = await this.api.sync(this.requireListId(), this.confirmedVersion);
      if (generation !== this.generation) {
        return;
      }
      if (response.kind === 'changes' && response.since !== this.confirmedVersion) {
        await this.loadSnapshot();
        return;
      }
      this.applySync(response);
      this.syncProblem.set(false);
    } catch (error) {
      if (generation !== this.generation) {
        return;
      }
      const kind = toApiError(error).kind;
      if (kind === 'not-found') {
        this.pending.set([]);
        this.state.set('not-found');
      } else if (kind !== 'unauthorized') {
        this.syncProblem.set(true);
      }
    }
  }

  private applySync(response: SyncResponse): void {
    if (response.kind === 'snapshot') {
      this.confirmed.set(response.items);
    } else {
      this.confirmed.update((items) => applyChanges(items, response.changed, response.removed));
    }
    this.confirmedVersion = response.version;
    this.list.set(response.list);
  }

  /** The displayed item if it can be changed now (exists and is confirmed by the server). */
  private editableItem(itemId: string): ShoppingItem | null {
    if (this.state() !== 'ready' || isLocalItem(itemId)) {
      return null;
    }
    return this.items().find((item) => item.id === itemId) ?? null;
  }

  /** The item as the server last confirmed it; missing means it was removed concurrently. */
  private confirmedItem(itemId: string): ShoppingItem {
    const item = this.confirmed().find((candidate) => candidate.id === itemId);
    if (item === undefined) {
      throw new ApiError('conflict', 409);
    }
    return item;
  }

  private requireListId(): string {
    if (this.listId === null) {
      throw new ApiError('unexpected', 0);
    }
    return this.listId;
  }

  private enqueue(job: () => Promise<void>): Promise<void> {
    this.queue = this.queue.then(job).catch(() => undefined);
    return this.queue;
  }
}

function failureText(error: ApiError): string {
  switch (error.kind) {
    case 'forbidden':
      return 'Dafür fehlt dir die Berechtigung.';
    case 'rate-limited':
      return 'Zu viele Änderungen in kurzer Zeit. Bitte warte einen Moment.';
    case 'invalid':
    case 'too-large':
      return 'Die Eingabe ist ungültig.';
    default:
      return 'Die Änderung konnte nicht gespeichert werden. Bitte versuche es erneut.';
  }
}
