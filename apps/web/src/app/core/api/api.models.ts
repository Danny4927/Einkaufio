/** Transport types mirroring contracts/openapi/openapi.yaml (v1.0.0). */

export type ListType = 'PERSONAL' | 'HOUSEHOLD';

export interface ShoppingListSummary {
  readonly id: string;
  readonly name: string;
  readonly type: ListType;
  /** Present exactly for household lists. */
  readonly householdId?: string;
  readonly version: number;
}

export interface ListPage {
  readonly items: readonly ShoppingListSummary[];
  readonly nextCursor?: string;
}

export interface ShoppingItem {
  readonly id: string;
  readonly name: string;
  readonly quantity?: string;
  readonly unit?: string;
  readonly note?: string;
  readonly checked: boolean;
  readonly position: number;
  readonly itemVersion: number;
}

/** Editable item details; a missing optional field clears the stored value (full replacement). */
export interface ItemDetails {
  readonly name: string;
  readonly quantity?: string;
  readonly unit?: string;
  readonly note?: string;
}

export interface ItemMutationResult {
  readonly item: ShoppingItem;
  readonly listVersion: number;
}

export interface ListVersionResult {
  readonly listVersion: number;
}

export interface ReorderResult {
  readonly items: readonly ShoppingItem[];
  readonly listVersion: number;
}

export interface SyncSnapshot {
  readonly kind: 'snapshot';
  readonly version: number;
  readonly list: ShoppingListSummary;
  readonly items: readonly ShoppingItem[];
}

export interface SyncChanges {
  readonly kind: 'changes';
  readonly version: number;
  readonly since: number;
  readonly list: ShoppingListSummary;
  readonly changed: readonly ShoppingItem[];
  readonly removed: readonly string[];
}

export type SyncResponse = SyncSnapshot | SyncChanges;

export const LIST_NAME_MAX_LENGTH = 120;
export const ITEM_NAME_MAX_LENGTH = 120;
export const ITEM_QUANTITY_MAX_LENGTH = 40;
export const ITEM_UNIT_MAX_LENGTH = 40;
export const ITEM_NOTE_MAX_LENGTH = 500;
