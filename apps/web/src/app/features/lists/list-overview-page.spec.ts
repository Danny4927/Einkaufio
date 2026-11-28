import { ComponentFixture, TestBed } from '@angular/core/testing';
import { provideRouter, Router } from '@angular/router';
import { ApiError } from '../../core/api/api-error';
import { ListPage } from '../../core/api/api.models';
import { ListsApiService } from '../../core/api/lists-api.service';
import { ListOverviewPage } from './list-overview-page';

describe('ListOverviewPage', () => {
  let api: { listLists: ReturnType<typeof vi.fn>; createList: ReturnType<typeof vi.fn> };
  let fixture: ComponentFixture<ListOverviewPage>;
  let router: Router;

  async function render(firstPage: ListPage | ApiError) {
    api = {
      listLists: vi.fn(async () => {
        if (firstPage instanceof ApiError) {
          throw firstPage;
        }
        return firstPage;
      }),
      createList: vi.fn(),
    };
    TestBed.configureTestingModule({
      imports: [ListOverviewPage],
      providers: [provideRouter([]), { provide: ListsApiService, useValue: api }],
    });
    router = TestBed.inject(Router);
    vi.spyOn(router, 'navigate').mockResolvedValue(true);
    fixture = TestBed.createComponent(ListOverviewPage);
    await fixture.whenStable();
  }

  const element = () => fixture.nativeElement as HTMLElement;
  const text = () => element().textContent ?? '';

  async function submit(name: string) {
    const input = element().querySelector<HTMLInputElement>('input[name="name"]');
    if (input === null) {
      throw new Error('name input missing');
    }
    input.value = name;
    element().querySelector('form')?.dispatchEvent(new SubmitEvent('submit', { cancelable: true }));
    await fixture.whenStable();
  }

  it('shows the empty state', async () => {
    await render({ items: [] });
    expect(text()).toContain('Noch keine Listen');
  });

  it('lists accessible lists with type labels and links', async () => {
    await render({
      items: [
        { id: 'l1', name: 'Wocheneinkauf', type: 'PERSONAL', version: 3 },
        { id: 'l2', name: 'WG-Küche', type: 'HOUSEHOLD', householdId: 'h1', version: 1 },
      ],
    });
    const links = [...element().querySelectorAll<HTMLAnchorElement>('a.list-card')];
    expect(links.map((link) => link.getAttribute('href'))).toEqual(['/listen/l1', '/listen/l2']);
    expect(text()).toContain('Persönlich');
    expect(text()).toContain('Haushalt');
  });

  it('loads further pages with the cursor', async () => {
    await render({ items: [{ id: 'l1', name: 'Erste', type: 'PERSONAL', version: 1 }], nextCursor: 'c1' });
    api.listLists.mockResolvedValueOnce({ items: [{ id: 'l2', name: 'Zweite', type: 'PERSONAL', version: 1 }] });
    element().querySelector<HTMLButtonElement>('.more button')?.click();
    await fixture.whenStable();
    expect(api.listLists).toHaveBeenLastCalledWith('c1');
    expect(text()).toContain('Zweite');
    expect(element().querySelector('.more')).toBeNull();
  });

  it('shows an error with retry when loading fails', async () => {
    await render(new ApiError('unavailable', 503));
    expect(text()).toContain('Listen konnten nicht geladen werden');
    api.listLists.mockResolvedValueOnce({ items: [] });
    element().querySelector<HTMLButtonElement>('.state-card button')?.click();
    await fixture.whenStable();
    expect(text()).toContain('Noch keine Listen');
  });

  it('creates a list and opens it', async () => {
    await render({ items: [] });
    api.createList.mockResolvedValueOnce({ id: 'neu', name: 'Party', type: 'PERSONAL', version: 1 });
    await submit('  Party ');
    expect(api.createList).toHaveBeenCalledWith('Party');
    expect(router.navigate).toHaveBeenCalledWith(['/listen', 'neu']);
  });

  it('validates the name before calling the API', async () => {
    await render({ items: [] });
    await submit('   ');
    expect(api.createList).not.toHaveBeenCalled();
    expect(text()).toContain('Bitte gib einen Namen mit 1 bis 120 Zeichen ein.');
  });

  it('reports a failed creation', async () => {
    await render({ items: [] });
    api.createList.mockRejectedValueOnce(new ApiError('rate-limited', 429));
    await submit('Party');
    expect(text()).toContain('Zu viele Anfragen');
    expect(router.navigate).not.toHaveBeenCalled();
  });
});
