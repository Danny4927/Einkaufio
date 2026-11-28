import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { Router } from '@angular/router';
import { AuthService } from '../../core/auth/auth.service';

/** Target of the Cognito redirect; completes the code exchange and returns to the app. */
@Component({
  selector: 'einkaufio-auth-callback-page',
  templateUrl: './auth-callback-page.html',
  styleUrl: './auth-callback-page.css',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class AuthCallbackPage {
  private readonly auth = inject(AuthService);
  private readonly router = inject(Router);

  protected readonly failed = signal(false);

  constructor() {
    void this.complete();
  }

  protected retry(): void {
    void this.auth.signIn();
  }

  private async complete(): Promise<void> {
    const signedIn = await this.auth.initialize();
    if (signedIn) {
      await this.router.navigateByUrl(this.auth.consumeReturnUrl(), { replaceUrl: true });
    } else {
      this.failed.set(true);
    }
  }
}
