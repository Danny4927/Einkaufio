import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { RUNTIME_CONFIG } from '../config/runtime-config';
import { TEST_RUNTIME_CONFIG } from '../testing/test-config';
import { ApiError } from './api-error';
import { ShoppingItem } from './api.models';
import { ListsApiService } from './lists-api.service';

const API = 'https://api.example.test/api';
const item: ShoppingItem = { id: 'i1', name: 'Milch', checked: false, position: 0, itemVersion: 1 };

describe('ListsApiService', () => {
  let api: ListsApiService;
  let http: HttpTestingController;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [provideHttpClient(), provideHttpClientTesting(), { provide: RUNTIME_CONFIG, useValue: TEST_RUNTIME_CONFIG }],
    });
    api = TestBed.inject(ListsApiService);
    http = TestBed.inject(HttpTestingController);
  });

  afterEach(() => http.verify());

  it('lists lists with an optional cursor', async () => {
    const first = api.listLists();
    http.expectOne(`${API}/lists`).flush({ items: [], nextCursor: 'c1' });
    await expect(first).resolves.toEqual({ items: [], nextCursor: 'c1' });

    const second = api.listLists('c1');
    const request = http.expectOne((r) => r.url === `${API}/lists`);
    expect(request.request.params.get('cursor')).toBe('c1');
    request.flush({ items: [] });
    await second;
  });

  it('creates a list with only the name (no owner id)', async () => {
    const created = api.createList('Wocheneinkauf');
    const request = http.expectOne(`${API}/lists`);
    expect(request.request.method).toBe('POST');
    expect(request.request.body).toEqual({ name: 'Wocheneinkauf' });
    request.flush({ id: 'l1', name: 'Wocheneinkauf', type: 'PERSONAL', version: 1 });
    await expect(created).resolves.toMatchObject({ id: 'l1' });
  });

  it('syncs with and without since and encodes path segments', async () => {
    const snapshot = api.sync('a/b');
    const first = http.expectOne(`${API}/lists/a%2Fb/sync`);
    expect(first.request.params.has('since')).toBe(false);
    first.flush({ kind: 'snapshot', version: 1, list: {}, items: [] });
    await snapshot;

    const changes = api.sync('l1', 7);
    const second = http.expectOne((r) => r.url === `${API}/lists/l1/sync`);
    expect(second.request.params.get('since')).toBe('7');
    second.flush({ kind: 'changes', version: 7, since: 7, list: {}, changed: [], removed: [] });
    await changes;
  });

  it('sends item mutations with the expected versions from the contract', async () => {
    const added = api.addItem('l1', 3, { name: 'Milch', quantity: '1', unit: 'l' });
    const add = http.expectOne(`${API}/lists/l1/items`);
    expect(add.request.body).toEqual({ expectedListVersion: 3, name: 'Milch', quantity: '1', unit: 'l' });
    add.flush({ item, listVersion: 4 });
    await added;

    const checked = api.setItemChecked('l1', 'i1', 1, true);
    const check = http.expectOne(`${API}/lists/l1/items/i1`);
    expect(check.request.method).toBe('PATCH');
    expect(check.request.body).toEqual({ expectedVersion: 1, checked: true });
    check.flush({ item, listVersion: 5 });
    await checked;

    const updated = api.updateItem('l1', 'i1', 2, { name: 'Hafermilch' });
    const update = http.expectOne(`${API}/lists/l1/items/i1`);
    expect(update.request.body).toEqual({ expectedVersion: 2, name: 'Hafermilch' });
    update.flush({ item: { ...item, name: 'Hafermilch', itemVersion: 3 }, listVersion: 6 });
    await expect(updated).resolves.toMatchObject({ listVersion: 6 });
  });

  it('deletes with expectedVersion as query and reorders with the full order', async () => {
    const deleted = api.deleteItem('l1', 'i1', 4);
    const del = http.expectOne((r) => r.url === `${API}/lists/l1/items/i1`);
    expect(del.request.method).toBe('DELETE');
    expect(del.request.params.get('expectedVersion')).toBe('4');
    del.flush({ listVersion: 9 });
    await expect(deleted).resolves.toEqual({ listVersion: 9 });

    const reordered = api.reorderItems('l1', 9, ['i2', 'i1']);
    const put = http.expectOne(`${API}/lists/l1/item-order`);
    expect(put.request.method).toBe('PUT');
    expect(put.request.body).toEqual({ expectedListVersion: 9, itemIds: ['i2', 'i1'] });
    put.flush({ items: [], listVersion: 10 });
    await reordered;
  });

  it('rejects with a typed ApiError', async () => {
    const result = api.setItemChecked('l1', 'i1', 1, true);
    http.expectOne(`${API}/lists/l1/items/i1`).flush({ error: { code: 'conflict', message: 'x' } }, { status: 409, statusText: 'Conflict' });
    const error = await result.catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect(error).toMatchObject({ kind: 'conflict', status: 409, code: 'conflict' });
  });
});
