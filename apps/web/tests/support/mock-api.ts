import type { Page, Route } from '@playwright/test';
import { APP_ORIGIN } from './mock-idp';

export const API_ORIGIN = 'https://api.einkaufio.test';

export interface MockItem {
  id: string;
  name: string;
  quantity?: string;
  unit?: string;
  note?: string;
  checked: boolean;
  position: number;
  itemVersion: number;
}

interface MockList {
  id: string;
  name: string;
  type: 'PERSONAL' | 'HOUSEHOLD';
  householdId?: string;
  version: number;
  items: Map<string, { item: MockItem; changedAt: number }>;
  removed: Map<string, number>;
}

export interface RecordedRequest {
  readonly method: string;
  readonly path: string;
  readonly search: URLSearchParams;
  readonly authorization: string | null;
}

interface Failure {
  readonly method: string;
  readonly pathSuffix: string;
  readonly status: number;
}

const CORS = {
  'access-control-allow-origin': APP_ORIGIN,
  'access-control-allow-headers': 'authorization, content-type',
  'access-control-allow-methods': 'GET, POST, PUT, PATCH, DELETE',
};

/**
 * In-memory API following contracts/openapi/openapi.yaml for list, item and sync routes,
 * including optimistic-concurrency 409s and `sync?since=` change sets.
 */
export class MockApi {
  readonly requests: RecordedRequest[] = [];
  private readonly lists = new Map<string, MockList>();
  private readonly failures: Failure[] = [];
  private nextList = 1;
  private nextItem = 1;

  constructor(private readonly accessToken: string) {}

  async install(page: Page): Promise<void> {
    await page.route(`${API_ORIGIN}/**`, (route) => this.handle(route));
  }

  /** Seeds a list as if created earlier; items are created in order. */
  seedList(name: string, items: (string | Partial<MockItem>)[] = [], type: MockList['type'] = 'PERSONAL'): string {
    const id = `00000000-0000-4000-8000-${String(this.nextList++).padStart(12, '0')}`;
    const list: MockList = {
      id,
      name,
      type,
      ...(type === 'HOUSEHOLD' ? { householdId: '10000000-0000-4000-8000-000000000001' } : {}),
      version: 1,
      items: new Map(),
      removed: new Map(),
    };
    this.lists.set(id, list);
    for (const entry of items) {
      this.otherClientAdds(id, typeof entry === 'string' ? { name: entry } : entry);
    }
    return id;
  }

  otherClientAdds(listId: string, details: Partial<MockItem>): MockItem {
    const list = this.require(listId);
    list.version += 1;
    const item: MockItem = {
      id: `item-${this.nextItem++}`,
      name: details.name ?? 'Artikel',
      ...(details.quantity ? { quantity: details.quantity } : {}),
      ...(details.unit ? { unit: details.unit } : {}),
      ...(details.note ? { note: details.note } : {}),
      checked: details.checked ?? false,
      position: list.items.size,
      itemVersion: 1,
    };
    list.items.set(item.id, { item, changedAt: list.version });
    return item;
  }

  otherClientChecks(listId: string, itemId: string): void {
    const list = this.require(listId);
    const stored = list.items.get(itemId);
    if (!stored) {
      throw new Error(`unknown item ${itemId}`);
    }
    list.version += 1;
    list.items.set(itemId, { item: { ...stored.item, checked: !stored.item.checked, itemVersion: stored.item.itemVersion + 1 }, changedAt: list.version });
  }

  /** The next request with this method whose path ends with `pathSuffix` fails with `status`. */
  failNext(method: string, pathSuffix: string, status: number): void {
    this.failures.push({ method, pathSuffix, status });
  }

  items(listId: string): MockItem[] {
    return [...this.require(listId).items.values()].map((stored) => stored.item).sort((a, b) => a.position - b.position);
  }

  listNames(): string[] {
    return [...this.lists.values()].map((list) => list.name);
  }

  syncRequests(listId: string): RecordedRequest[] {
    return this.requests.filter((request) => request.method === 'GET' && request.path === `/api/lists/${listId}/sync`);
  }

  private async handle(route: Route): Promise<void> {
    const request = route.request();
    const url = new URL(request.url());
    if (request.method() === 'OPTIONS') {
      return route.fulfill({ status: 204, headers: CORS });
    }
    const authorization = request.headers()['authorization'] ?? null;
    this.requests.push({ method: request.method(), path: url.pathname, search: url.searchParams, authorization });
    if (authorization !== `Bearer ${this.accessToken}`) {
      return this.error(route, 401, 'unauthorized');
    }
    const failureIndex = this.failures.findIndex((f) => f.method === request.method() && url.pathname.endsWith(f.pathSuffix));
    if (failureIndex !== -1) {
      const [failure] = this.failures.splice(failureIndex, 1);
      return this.error(route, failure.status, 'injected_failure');
    }
    const body = request.postDataJSON() as Record<string, unknown> | null;
    const segments = url.pathname.split('/').filter(Boolean);
    const [, resource, listId, sub, itemId] = segments;
    if (segments[0] !== 'api' || resource !== 'lists') {
      return this.error(route, 404, 'not_found');
    }
    if (listId === undefined) {
      if (request.method() === 'GET') {
        return route.fulfill({ headers: CORS, json: { items: [...this.lists.values()].map(summary) } });
      }
      if (request.method() === 'POST') {
        const name = String(body?.['name'] ?? '').trim();
        if (name.length === 0 || name.length > 120) {
          return this.error(route, 400, 'invalid_input');
        }
        const id = this.seedList(name);
        return route.fulfill({ status: 201, headers: CORS, json: summary(this.require(id)) });
      }
    }
    const list = listId === undefined ? undefined : this.lists.get(listId);
    if (list === undefined) {
      return this.error(route, 404, 'not_found');
    }
    if (sub === 'sync' && request.method() === 'GET') {
      return route.fulfill({ headers: CORS, json: this.sync(list, url.searchParams.get('since')) });
    }
    if (sub === 'items' && itemId === undefined && request.method() === 'POST') {
      if (body?.['expectedListVersion'] !== list.version) {
        return this.error(route, 409, 'version_conflict');
      }
      const item = this.otherClientAdds(list.id, body as Partial<MockItem>);
      return route.fulfill({ status: 201, headers: CORS, json: { item, listVersion: list.version } });
    }
    if (sub === 'items' && itemId !== undefined) {
      const stored = list.items.get(itemId);
      if (stored === undefined) {
        return this.error(route, 404, 'not_found');
      }
      const expected = request.method() === 'DELETE' ? Number(url.searchParams.get('expectedVersion')) : body?.['expectedVersion'];
      if (expected !== stored.item.itemVersion) {
        return this.error(route, 409, 'version_conflict');
      }
      list.version += 1;
      if (request.method() === 'DELETE') {
        list.items.delete(itemId);
        list.removed.set(itemId, list.version);
        return route.fulfill({ headers: CORS, json: { listVersion: list.version } });
      }
      const { id, checked, position, itemVersion } = stored.item;
      const item: MockItem =
        typeof body?.['checked'] === 'boolean'
          ? { ...stored.item, checked: body['checked'], itemVersion: itemVersion + 1 }
          : {
              id,
              checked,
              position,
              itemVersion: itemVersion + 1,
              name: String(body?.['name']),
              ...(body?.['quantity'] ? { quantity: String(body['quantity']) } : {}),
              ...(body?.['unit'] ? { unit: String(body['unit']) } : {}),
              ...(body?.['note'] ? { note: String(body['note']) } : {}),
            };
      list.items.set(itemId, { item, changedAt: list.version });
      return route.fulfill({ headers: CORS, json: { item, listVersion: list.version } });
    }
    if (sub === 'item-order' && request.method() === 'PUT') {
      const ids = (body?.['itemIds'] ?? []) as string[];
      if (body?.['expectedListVersion'] !== list.version) {
        return this.error(route, 409, 'version_conflict');
      }
      if (ids.length !== list.items.size || ids.some((id) => !list.items.has(id))) {
        return this.error(route, 400, 'invalid_input');
      }
      list.version += 1;
      ids.forEach((id, position) => {
        const stored = list.items.get(id);
        if (stored && stored.item.position !== position) {
          list.items.set(id, { item: { ...stored.item, position, itemVersion: stored.item.itemVersion + 1 }, changedAt: list.version });
        }
      });
      return route.fulfill({ headers: CORS, json: { items: this.items(list.id), listVersion: list.version } });
    }
    return this.error(route, 404, 'not_found');
  }

  private sync(list: MockList, since: string | null) {
    const base = { version: list.version, list: summary(list) };
    if (since === null) {
      return { kind: 'snapshot', ...base, items: this.items(list.id) };
    }
    const after = Number(since);
    return {
      kind: 'changes',
      ...base,
      since: after,
      changed: [...list.items.values()].filter((stored) => stored.changedAt > after).map((stored) => stored.item),
      removed: [...list.removed.entries()].filter(([, at]) => at > after).map(([id]) => id),
    };
  }

  private require(listId: string): MockList {
    const list = this.lists.get(listId);
    if (!list) {
      throw new Error(`unknown list ${listId}`);
    }
    return list;
  }

  private error(route: Route, status: number, code: string): Promise<void> {
    return route.fulfill({ status, headers: CORS, json: { error: { code, message: code } } });
  }
}

function summary(list: MockList) {
  return {
    id: list.id,
    name: list.name,
    type: list.type,
    ...(list.householdId ? { householdId: list.householdId } : {}),
    version: list.version,
  };
}
