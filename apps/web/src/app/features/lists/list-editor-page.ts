import { ChangeDetectionStrategy, Component, DestroyRef, effect, inject, input, signal, untracked } from '@angular/core';
import { RouterLink } from '@angular/router';
import {
  ITEM_NAME_MAX_LENGTH,
  ITEM_NOTE_MAX_LENGTH,
  ITEM_QUANTITY_MAX_LENGTH,
  ITEM_UNIT_MAX_LENGTH,
  ShoppingItem,
} from '../../core/api/api.models';
import { VisiblePoller } from '../../core/sync/visible-poller';
import { isLocalItem, ItemDetailsInput, normalizeItemDetails } from './item-ops';
import { ListEditorStore } from './list-editor.store';

const INVALID_ITEM_TEXT = `Bitte gib einen Artikel mit 1 bis ${ITEM_NAME_MAX_LENGTH} Zeichen ein.`;

@Component({
  selector: 'einkaufio-list-editor-page',
  imports: [RouterLink],
  templateUrl: './list-editor-page.html',
  styleUrl: './list-editor-page.css',
  providers: [ListEditorStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ListEditorPage {
  /** Bound from the `:listId` route parameter. */
  readonly listId = input.required<string>();

  protected readonly store = inject(ListEditorStore);
  protected readonly limits = {
    name: ITEM_NAME_MAX_LENGTH,
    quantity: ITEM_QUANTITY_MAX_LENGTH,
    unit: ITEM_UNIT_MAX_LENGTH,
    note: ITEM_NOTE_MAX_LENGTH,
  };
  protected readonly editingId = signal<string | null>(null);
  protected readonly addError = signal<string | null>(null);
  protected readonly editError = signal<string | null>(null);

  constructor() {
    effect(() => {
      const listId = this.listId();
      untracked(() => {
        this.editingId.set(null);
        void this.store.open(listId);
      });
    });
    // Only the list that is open in this editor polls; leaving the page stops it.
    const poller = inject(VisiblePoller).start(() => void this.store.refresh());
    inject(DestroyRef).onDestroy(() => poller.stop());
  }

  protected isLocal(item: ShoppingItem): boolean {
    return isLocalItem(item.id);
  }

  protected amount(item: ShoppingItem): string {
    return [item.quantity, item.unit].filter((part) => part !== undefined && part !== '').join(' ');
  }

  protected add(event: SubmitEvent, form: HTMLFormElement): void {
    event.preventDefault();
    const details = normalizeItemDetails(readDetails(form));
    if (details === null) {
      this.addError.set(INVALID_ITEM_TEXT);
      return;
    }
    this.addError.set(null);
    this.store.addItem(details);
    form.reset();
    form.querySelector<HTMLInputElement>('input[name="name"]')?.focus();
  }

  protected startEdit(item: ShoppingItem): void {
    this.editError.set(null);
    this.editingId.set(item.id);
  }

  protected cancelEdit(): void {
    this.editingId.set(null);
  }

  protected saveEdit(event: SubmitEvent, form: HTMLFormElement, itemId: string): void {
    event.preventDefault();
    const details = normalizeItemDetails(readDetails(form));
    if (details === null) {
      this.editError.set(INVALID_ITEM_TEXT);
      return;
    }
    this.store.updateItem(itemId, details);
    this.editingId.set(null);
  }

  protected toggle(item: ShoppingItem): void {
    this.store.toggleItem(item.id);
  }

  protected remove(item: ShoppingItem): void {
    if (this.editingId() === item.id) {
      this.editingId.set(null);
    }
    this.store.deleteItem(item.id);
  }

  protected move(item: ShoppingItem, delta: -1 | 1): void {
    this.store.moveItem(item.id, delta);
  }

  protected retry(): void {
    void this.store.open(this.listId());
  }
}

function readDetails(form: HTMLFormElement): ItemDetailsInput {
  const data = new FormData(form);
  const value = (key: string) => {
    const entry = data.get(key);
    return typeof entry === 'string' ? entry : undefined;
  };
  return { name: value('name') ?? '', quantity: value('quantity'), unit: value('unit'), note: value('note') };
}
