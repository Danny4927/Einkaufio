import {
  ITEM_NAME_MAX_LENGTH,
  ITEM_NOTE_MAX_LENGTH,
  ITEM_QUANTITY_MAX_LENGTH,
  ITEM_UNIT_MAX_LENGTH,
  ItemDetails,
  ShoppingItem,
} from '../../core/api/api.models';

/** Prefix of client-generated ids for items whose creation is not yet confirmed. */
export const LOCAL_ITEM_PREFIX = 'lokal-';

export function isLocalItem(itemId: string): boolean {
  return itemId.startsWith(LOCAL_ITEM_PREFIX);
}

export function sortByPosition(items: readonly ShoppingItem[]): ShoppingItem[] {
  return [...items].sort((left, right) => left.position - right.position || left.id.localeCompare(right.id));
}

export function upsertItem(items: readonly ShoppingItem[], item: ShoppingItem): ShoppingItem[] {
  const index = items.findIndex((candidate) => candidate.id === item.id);
  if (index === -1) {
    return [...items, item];
  }
  return items.map((candidate, candidateIndex) => (candidateIndex === index ? item : candidate));
}

export function removeItem(items: readonly ShoppingItem[], itemId: string): ShoppingItem[] {
  return items.filter((item) => item.id !== itemId);
}

export function replaceItem(
  items: readonly ShoppingItem[],
  itemId: string,
  change: (item: ShoppingItem) => ShoppingItem,
): ShoppingItem[] {
  return items.map((item) => (item.id === itemId ? change(item) : item));
}

/** Applies a `changes` sync result: removals first, then changed items replace by id. */
export function applyChanges(
  items: readonly ShoppingItem[],
  changed: readonly ShoppingItem[],
  removed: readonly string[],
): ShoppingItem[] {
  const removedIds = new Set(removed);
  return changed.reduce(upsertItem, items.filter((item) => !removedIds.has(item.id)));
}

export function nextPosition(items: readonly ShoppingItem[]): number {
  return items.reduce((max, item) => Math.max(max, item.position + 1), 0);
}

/**
 * Whether `itemId` can move by `delta` in position order. Unconfirmed items neither move nor
 * get swapped with, because the server does not know them yet.
 */
export function canMove(items: readonly ShoppingItem[], itemId: string, delta: -1 | 1): boolean {
  const ordered = sortByPosition(items);
  const index = ordered.findIndex((item) => item.id === itemId);
  const target = ordered[index + delta];
  return index !== -1 && target !== undefined && !isLocalItem(itemId) && !isLocalItem(target.id);
}

/** Moves one item by `delta` and renumbers positions 0..n-1; unchanged when impossible. */
export function moveItem(items: readonly ShoppingItem[], itemId: string, delta: -1 | 1): ShoppingItem[] {
  const ordered = sortByPosition(items);
  const index = ordered.findIndex((item) => item.id === itemId);
  const targetIndex = index + delta;
  if (index === -1 || targetIndex < 0 || targetIndex >= ordered.length) {
    return ordered;
  }
  const [moved] = ordered.splice(index, 1);
  ordered.splice(targetIndex, 0, moved);
  return ordered.map((item, position) => (item.position === position ? item : { ...item, position }));
}

/** Raw form values for item details. */
export interface ItemDetailsInput {
  readonly name: string;
  readonly quantity?: string;
  readonly unit?: string;
  readonly note?: string;
}

/** Trims input, drops empty optional fields and enforces contract lengths; null when invalid. */
export function normalizeItemDetails(input: ItemDetailsInput): ItemDetails | null {
  const name = input.name.trim();
  if (name.length === 0 || name.length > ITEM_NAME_MAX_LENGTH) {
    return null;
  }
  const quantity = optional(input.quantity, ITEM_QUANTITY_MAX_LENGTH);
  const unit = optional(input.unit, ITEM_UNIT_MAX_LENGTH);
  const note = optional(input.note, ITEM_NOTE_MAX_LENGTH);
  if (quantity === false || unit === false || note === false) {
    return null;
  }
  return {
    name,
    ...(quantity === undefined ? {} : { quantity }),
    ...(unit === undefined ? {} : { unit }),
    ...(note === undefined ? {} : { note }),
  };
}

function optional(value: string | undefined, maxLength: number): string | undefined | false {
  const trimmed = value?.trim() ?? '';
  if (trimmed.length === 0) {
    return undefined;
  }
  return trimmed.length > maxLength ? false : trimmed;
}
