import { ComponentFixture, TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { ListsApiService } from '../../core/api/lists-api.service';
import { VisiblePoller } from '../../core/sync/visible-poller';
import { ListEditorPage } from './list-editor-page';
import { ListEditorStore } from './list-editor.store';
import { FakeListsApi } from './testing/fake-lists-api';

describe('ListEditorPage', () => {
  let api: FakeListsApi;
  let fixture: ComponentFixture<ListEditorPage>;
  let poll: () => void;
  const stop = vi.fn();

  async function render() {
    TestBed.configureTestingModule({
      imports: [ListEditorPage],
      providers: [
        provideRouter([]),
        { provide: ListsApiService, useValue: api },
        {
          provide: VisiblePoller,
          useValue: {
            start: (task: () => void) => {
              poll = task;
              return { stop };
            },
          },
        },
      ],
    });
    fixture = TestBed.createComponent(ListEditorPage);
    fixture.componentRef.setInput('listId', 'l1');
    await settle();
  }

  async function settle() {
    await fixture.whenStable();
    await fixture.debugElement.injector.get(ListEditorStore).whenIdle();
    await fixture.whenStable();
  }

  const element = () => fixture.nativeElement as HTMLElement;
  const text = () => element().textContent ?? '';
  const button = (label: string) => {
    const found = element().querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`);
    if (found === null) {
      throw new Error(`button ${label} missing`);
    }
    return found;
  };
  const names = () => [...element().querySelectorAll('.item-name')].map((node) => node.textContent?.trim());

  async function submitForm(form: HTMLFormElement, values: Record<string, string>) {
    for (const [name, value] of Object.entries(values)) {
      const field = form.querySelector<HTMLInputElement | HTMLTextAreaElement>(`[name="${name}"]`);
      if (field === null) {
        throw new Error(`field ${name} missing`);
      }
      field.value = value;
    }
    form.dispatchEvent(new SubmitEvent('submit', { cancelable: true }));
    await settle();
  }

  beforeEach(() => {
    api = new FakeListsApi();
    stop.mockClear();
  });

  it('renders the list with progress and amounts', async () => {
    api.seed('Milch', 'Brot');
    await render();
    expect(element().querySelector('h1')?.textContent).toContain('Wocheneinkauf');
    expect(names()).toEqual(['Milch', 'Brot']);
    expect(text()).toContain('0 von 2 erledigt');
  });

  it('shows the empty state', async () => {
    await render();
    expect(text()).toContain('Diese Liste ist noch leer');
  });

  it('adds an item from the form and validates the name', async () => {
    await render();
    const form = element().querySelector<HTMLFormElement>('form.add-form');
    if (form === null) {
      throw new Error('add form missing');
    }
    await submitForm(form, { name: '  ' });
    expect(text()).toContain('Bitte gib einen Artikel');
    await submitForm(form, { name: 'Hafermilch', quantity: '2', unit: 'l' });
    expect(api.serverItems()).toMatchObject([{ name: 'Hafermilch', quantity: '2', unit: 'l' }]);
    expect(text()).toContain('2 l');
  });

  it('checks items and shows the completed banner', async () => {
    api.seed('Milch');
    await render();
    element().querySelector<HTMLInputElement>('input[type="checkbox"]')?.dispatchEvent(new Event('change'));
    await settle();
    expect(api.serverItems()[0].checked).toBe(true);
    expect(text()).toContain('Alles erledigt.');
  });

  it('edits and deletes items', async () => {
    api.seed('Milch', 'Brot');
    await render();
    button('„Milch“ bearbeiten').click();
    await fixture.whenStable();
    const form = element().querySelector<HTMLFormElement>('form.edit-form');
    if (form === null) {
      throw new Error('edit form missing');
    }
    await submitForm(form, { name: 'Hafermilch', note: 'ungesüßt' });
    expect(api.serverItems()[0]).toMatchObject({ name: 'Hafermilch', note: 'ungesüßt' });
    button('„Brot“ löschen').click();
    await settle();
    expect(names()).toEqual(['Hafermilch']);
  });

  it('cancels an edit without saving', async () => {
    api.seed('Milch');
    await render();
    button('„Milch“ bearbeiten').click();
    await fixture.whenStable();
    [...element().querySelectorAll<HTMLButtonElement>('form.edit-form button')].find((b) => b.textContent?.includes('Abbrechen'))?.click();
    await fixture.whenStable();
    expect(element().querySelector('form.edit-form')).toBeNull();
    expect(api.calls).toEqual(['sync l1']);
  });

  it('reorders with the move buttons and disables impossible moves', async () => {
    api.seed('Milch', 'Brot');
    await render();
    expect(button('„Milch“ nach oben verschieben').disabled).toBe(true);
    button('„Brot“ nach oben verschieben').click();
    await settle();
    expect(names()).toEqual(['Brot', 'Milch']);
    expect(api.serverItems().map((item) => item.name)).toEqual(['Brot', 'Milch']);
  });

  it('shows the conflict notice after a 409 and can dismiss it', async () => {
    const [milk] = api.seed('Milch');
    await render();
    api.otherClientChecks(milk.id);
    button('„Milch“ löschen').click();
    await settle();
    expect(text()).toContain('Die Liste wurde zwischenzeitlich geändert');
    expect(names()).toEqual(['Milch']);
    [...element().querySelectorAll<HTMLButtonElement>('.notice button')][0]?.click();
    await fixture.whenStable();
    expect(element().querySelector('.notice')).toBeNull();
  });

  it('polls through the visible poller and stops on destroy', async () => {
    await render();
    api.otherClientAdds('Tee');
    poll();
    await settle();
    expect(names()).toEqual(['Tee']);
    fixture.destroy();
    expect(stop).toHaveBeenCalled();
  });

  it('shows not-found and load errors with retry', async () => {
    api.missing = true;
    await render();
    expect(text()).toContain('Liste nicht gefunden');

    TestBed.resetTestingModule();
    api = new FakeListsApi();
    api.failNext('unavailable', 503);
    await render();
    expect(text()).toContain('Liste konnte nicht geladen werden');
    [...element().querySelectorAll<HTMLButtonElement>('.state-card button')][0]?.click();
    await settle();
    expect(text()).toContain('Diese Liste ist noch leer');
  });
});
