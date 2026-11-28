import { TestBed } from '@angular/core/testing';
import { ListsApiService } from '../../core/api/lists-api.service';
import { CONFLICT_NOTICE, ListEditorStore } from './list-editor.store';
import { FakeListsApi } from './testing/fake-lists-api';

describe('ListEditorStore', () => {
  let api: FakeListsApi;
  let store: ListEditorStore;

  const names = () => store.items().map((item) => item.name);

  beforeEach(async () => {
    api = new FakeListsApi();
    api.seed('Milch', 'Brot', 'Kaffee');
    TestBed.configureTestingModule({ providers: [ListEditorStore, { provide: ListsApiService, useValue: api }] });
    store = TestBed.inject(ListEditorStore);
    await store.open('l1');
    api.calls.length = 0;
  });

  it('opens a list from a full snapshot', () => {
    expect(store.state()).toBe('ready');
    expect(store.list()?.name).toBe('Wocheneinkauf');
    expect(names()).toEqual(['Milch', 'Brot', 'Kaffee']);
  });

  it('shows an added item immediately and confirms it without an extra sync', async () => {
    const release = api.hold();
    store.addItem({ name: 'Äpfel', quantity: '6' });
    expect(names()).toEqual(['Milch', 'Brot', 'Kaffee', 'Äpfel']);
    expect(store.items()[3].id).toMatch(/^lokal-/);
    expect(store.saving()).toBe(true);
    release();
    await store.whenIdle();
    expect(store.items()[3]).toMatchObject({ id: 'i4', name: 'Äpfel', quantity: '6' });
    expect(store.saving()).toBe(false);
    expect(api.calls).toEqual(['add Äpfel']);
  });

  it('checks, edits and deletes optimistically using the item version', async () => {
    const release = api.hold();
    store.toggleItem('i1');
    store.updateItem('i2', { name: 'Vollkornbrot', quantity: '1' });
    store.deleteItem('i3');
    expect(store.items()).toMatchObject([{ id: 'i1', checked: true }, { id: 'i2', name: 'Vollkornbrot', quantity: '1' }]);
    expect(store.checkedCount()).toBe(1);
    release();
    await store.whenIdle();
    expect(api.serverItems()).toMatchObject([{ id: 'i1', checked: true, itemVersion: 2 }, { id: 'i2', name: 'Vollkornbrot' }]);
    expect(store.items()).toEqual(api.serverItems());
    expect(api.calls).toEqual(['check i1=true', 'update i2', 'delete i3']);
  });

  it('clears optional details that an edit leaves empty', async () => {
    store.updateItem('i1', { name: 'Milch', quantity: '2' });
    await store.whenIdle();
    store.updateItem('i1', { name: 'Milch' });
    expect(store.items()[0].quantity).toBeUndefined();
    await store.whenIdle();
    expect(api.serverItems()[0].quantity).toBeUndefined();
  });

  it('reorders with the complete order and keeps positions consistent', async () => {
    store.moveItem('i3', -1);
    store.moveItem('i3', -1);
    expect(names()).toEqual(['Kaffee', 'Milch', 'Brot']);
    await store.whenIdle();
    expect(api.calls).toEqual(['reorder i1,i3,i2', 'reorder i3,i1,i2']);
    expect(store.items()).toEqual(api.serverItems());
    expect(store.canMove('i3', -1)).toBe(false);
  });

  it('reloads the snapshot and explains a 409 conflict', async () => {
    api.otherClientChecks('i1');
    const release = api.hold();
    store.toggleItem('i1');
    store.toggleItem('i2');
    release();
    await store.whenIdle();
    expect(store.notice()).toEqual({ kind: 'conflict', text: CONFLICT_NOTICE });
    expect(api.calls).toEqual(['check i1=true', 'sync l1']);
    // Server state wins: the other client's check stays, the dropped toggle of i2 is not sent.
    expect(store.items()).toEqual(api.serverItems());
    expect(store.items()[1].checked).toBe(false);
    expect(store.saving()).toBe(false);
  });

  it('treats an item deleted by someone else as a conflict', async () => {
    api.otherClientDeletes('i2');
    store.toggleItem('i2');
    await store.whenIdle();
    expect(store.notice()?.kind).toBe('conflict');
    expect(names()).toEqual(['Milch', 'Kaffee']);
  });

  it('rolls back only the failed change on other errors', async () => {
    api.failNext('unavailable', 503);
    store.deleteItem('i1');
    store.toggleItem('i2');
    await store.whenIdle();
    expect(names()).toEqual(['Milch', 'Brot', 'Kaffee']);
    expect(store.items()[1].checked).toBe(true);
    expect(store.notice()).toEqual({ kind: 'error', text: 'Die Änderung konnte nicht gespeichert werden. Bitte versuche es erneut.' });
  });

  it('polls only the changes since the known version', async () => {
    api.otherClientAdds('Tee');
    api.otherClientDeletes('i1');
    await store.refresh();
    expect(api.calls).toEqual(['sync l1 since=4']);
    expect(names()).toEqual(['Brot', 'Kaffee', 'Tee']);
  });

  it('keeps optimistic changes when another client changed the list in between', async () => {
    api.otherClientAdds('Tee');
    store.toggleItem('i1');
    await store.whenIdle();
    // The mutation result jumped two versions, so the store fetched what it missed.
    expect(api.calls).toEqual(['check i1=true', 'sync l1 since=4']);
    expect(names()).toEqual(['Milch', 'Brot', 'Kaffee', 'Tee']);
    expect(store.items()[0].checked).toBe(true);
  });

  it('skips polling while changes are pending so they are never overwritten', async () => {
    const release = api.hold();
    store.toggleItem('i1');
    await store.refresh();
    expect(api.calls).toEqual(['check i1=true']);
    expect(store.items()[0].checked).toBe(true);
    release();
    await store.whenIdle();
  });

  it('does not stack polls while one is in flight', async () => {
    const release = api.hold();
    const first = store.refresh();
    const second = store.refresh();
    release();
    await Promise.all([first, second]);
    expect(api.calls).toEqual(['sync l1 since=4']);
  });

  it('flags a failing poll and recovers on the next success', async () => {
    api.failNext('network', 0);
    await store.refresh();
    expect(store.syncProblem()).toBe(true);
    expect(store.state()).toBe('ready');
    await store.refresh();
    expect(store.syncProblem()).toBe(false);
  });

  it('switches to not-found when the list disappears', async () => {
    api.missing = true;
    await store.refresh();
    expect(store.state()).toBe('not-found');
  });

  it('reports load errors and not-found lists on open', async () => {
    api.failNext('unavailable', 503);
    await store.open('l1');
    expect(store.state()).toBe('error');
    api.missing = true;
    await store.open('l1');
    expect(store.state()).toBe('not-found');
  });

  it('ignores late responses for a list that is no longer open', async () => {
    const release = api.hold();
    store.toggleItem('i1');
    const reopened = store.open('l1');
    expect(store.state()).toBe('loading');
    release();
    await reopened;
    expect(store.state()).toBe('ready');
    expect(store.saving()).toBe(false);
  });

  it('ignores interactions with unconfirmed items', async () => {
    const release = api.hold();
    store.addItem({ name: 'Tee' });
    const local = store.items()[3].id;
    store.toggleItem(local);
    store.deleteItem(local);
    store.moveItem(local, -1);
    expect(store.items()[3]).toMatchObject({ id: local, checked: false });
    release();
    await store.whenIdle();
    expect(api.calls).toEqual(['add Tee']);
  });
});
