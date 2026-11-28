import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { Router, RouterLink } from '@angular/router';
import { ApiError, toApiError } from '../../core/api/api-error';
import { LIST_NAME_MAX_LENGTH, ListType, ShoppingListSummary } from '../../core/api/api.models';
import { ListsApiService } from '../../core/api/lists-api.service';

type OverviewState = 'loading' | 'ready' | 'error';

const TYPE_LABELS: Record<ListType, string> = { PERSONAL: 'Persönlich', HOUSEHOLD: 'Haushalt' };

@Component({
  selector: 'einkaufio-list-overview-page',
  imports: [RouterLink],
  templateUrl: './list-overview-page.html',
  styleUrl: './list-overview-page.css',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ListOverviewPage {
  private readonly api = inject(ListsApiService);
  private readonly router = inject(Router);

  protected readonly maxNameLength = LIST_NAME_MAX_LENGTH;
  protected readonly state = signal<OverviewState>('loading');
  protected readonly lists = signal<readonly ShoppingListSummary[]>([]);
  protected readonly nextCursor = signal<string | null>(null);
  protected readonly loadingMore = signal(false);
  protected readonly loadMoreFailed = signal(false);
  protected readonly creating = signal(false);
  protected readonly createError = signal<string | null>(null);

  constructor() {
    void this.load();
  }

  protected typeLabel(type: ListType): string {
    return TYPE_LABELS[type];
  }

  protected async load(): Promise<void> {
    this.state.set('loading');
    try {
      const page = await this.api.listLists();
      this.lists.set(page.items);
      this.nextCursor.set(page.nextCursor ?? null);
      this.state.set('ready');
    } catch {
      this.state.set('error');
    }
  }

  protected async loadMore(): Promise<void> {
    const cursor = this.nextCursor();
    if (cursor === null || this.loadingMore()) {
      return;
    }
    this.loadingMore.set(true);
    this.loadMoreFailed.set(false);
    try {
      const page = await this.api.listLists(cursor);
      this.lists.update((lists) => [...lists, ...page.items]);
      this.nextCursor.set(page.nextCursor ?? null);
    } catch {
      this.loadMoreFailed.set(true);
    } finally {
      this.loadingMore.set(false);
    }
  }

  protected async create(event: SubmitEvent, form: HTMLFormElement): Promise<void> {
    event.preventDefault();
    if (this.creating()) {
      return;
    }
    const name = String(new FormData(form).get('name') ?? '').trim();
    if (name.length === 0 || name.length > LIST_NAME_MAX_LENGTH) {
      this.createError.set(`Bitte gib einen Namen mit 1 bis ${LIST_NAME_MAX_LENGTH} Zeichen ein.`);
      return;
    }
    this.creating.set(true);
    this.createError.set(null);
    try {
      const list = await this.api.createList(name);
      form.reset();
      await this.router.navigate(['/listen', list.id]);
    } catch (error) {
      this.createError.set(createFailureText(toApiError(error)));
    } finally {
      this.creating.set(false);
    }
  }
}

function createFailureText(error: ApiError): string {
  switch (error.kind) {
    case 'invalid':
    case 'too-large':
      return 'Der Name ist ungültig.';
    case 'rate-limited':
      return 'Zu viele Anfragen. Bitte warte einen Moment.';
    default:
      return 'Die Liste konnte nicht angelegt werden. Bitte versuche es erneut.';
  }
}
