import { ShoppingItem } from '../../core/api/api.models';
import { applyChanges, canMove, moveItem, nextPosition, normalizeItemDetails, sortByPosition, upsertItem } from './item-ops';

const item = (id: string, position: number, extra: Partial<ShoppingItem> = {}): ShoppingItem => ({
  id,
  name: id,
  checked: false,
  position,
  itemVersion: 1,
  ...extra,
});

describe('item operations', () => {
  it('sorts by position with a stable id tie-break', () => {
    expect(sortByPosition([item('b', 1), item('c', 0), item('a', 1)]).map((i) => i.id)).toEqual(['c', 'a', 'b']);
  });

  it('upserts by id', () => {
    const items = [item('a', 0)];
    expect(upsertItem(items, item('a', 0, { name: 'neu' }))).toEqual([item('a', 0, { name: 'neu' })]);
    expect(upsertItem(items, item('b', 1)).map((i) => i.id)).toEqual(['a', 'b']);
  });

  it('applies removals and changes from a sync', () => {
    const result = applyChanges([item('a', 0), item('b', 1)], [item('b', 0, { checked: true }), item('c', 1)], ['a']);
    expect(result).toEqual([item('b', 0, { checked: true }), item('c', 1)]);
  });

  it('computes the next free position', () => {
    expect(nextPosition([])).toBe(0);
    expect(nextPosition([item('a', 4), item('b', 2)])).toBe(5);
  });

  it('moves an item and renumbers positions', () => {
    const moved = moveItem([item('a', 0), item('b', 1), item('c', 2)], 'c', -1);
    expect(moved.map((i) => [i.id, i.position])).toEqual([['a', 0], ['c', 1], ['b', 2]]);
    expect(moveItem([item('a', 0)], 'a', -1).map((i) => i.id)).toEqual(['a']);
  });

  it('refuses to move unconfirmed items or past the edges', () => {
    const items = [item('a', 0), item('b', 1), item('lokal-1', 2)];
    expect(canMove(items, 'a', 1)).toBe(true);
    expect(canMove(items, 'a', -1)).toBe(false);
    expect(canMove(items, 'b', 1)).toBe(false);
    expect(canMove(items, 'lokal-1', -1)).toBe(false);
    expect(canMove(items, 'missing', 1)).toBe(false);
  });

  it('normalizes item details from form input', () => {
    expect(normalizeItemDetails({ name: '  Milch ', quantity: ' 2 ', unit: '', note: '   ' })).toEqual({ name: 'Milch', quantity: '2' });
    expect(normalizeItemDetails({ name: '   ' })).toBeNull();
    expect(normalizeItemDetails({ name: 'x'.repeat(121) })).toBeNull();
    expect(normalizeItemDetails({ name: 'Milch', quantity: 'x'.repeat(41) })).toBeNull();
    expect(normalizeItemDetails({ name: 'Milch', note: 'x'.repeat(501) })).toBeNull();
  });
});
