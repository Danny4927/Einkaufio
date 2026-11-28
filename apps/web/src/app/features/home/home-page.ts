import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';
import { RouterLink } from '@angular/router';
import { AuthService } from '../../core/auth/auth.service';

@Component({
  selector: 'einkaufio-home-page',
  imports: [RouterLink],
  templateUrl: './home-page.html',
  styleUrl: './home-page.css',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class HomePage {
  /** `?anmeldung=erforderlich`: a protected route redirected here. */
  readonly anmeldung = input<string>();
  /** `?sitzung=abgelaufen`: the API rejected the token. */
  readonly sitzung = input<string>();

  protected readonly auth = inject(AuthService);
  protected readonly featureHighlights = [
    'Listen gemeinsam bearbeiten',
    'Änderungen synchron halten',
    'Mit Familie und Freunden teilen',
  ];
  protected readonly hint = computed(() => {
    if (this.auth.isAuthenticated()) {
      return null;
    }
    if (this.sitzung() === 'abgelaufen') {
      return 'Deine Sitzung ist abgelaufen. Bitte melde dich erneut an.';
    }
    if (this.anmeldung() === 'erforderlich') {
      return 'Bitte melde dich an, um deine Listen zu sehen.';
    }
    return null;
  });

  protected signIn(): void {
    void this.auth.signIn();
  }
}
