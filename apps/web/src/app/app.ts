import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { RouterLink, RouterOutlet } from '@angular/router';
import { AuthService } from './core/auth/auth.service';

@Component({
  selector: 'einkaufio-root',
  imports: [RouterOutlet, RouterLink],
  templateUrl: './app.html',
  styleUrl: './app.css',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class App {
  protected readonly auth = inject(AuthService);

  constructor() {
    // Runs before the router processes /auth/callback so the code exchange sees the URL.
    void this.auth.initialize();
  }

  protected signIn(): void {
    void this.auth.signIn();
  }

  protected signOut(): void {
    void this.auth.signOut();
  }
}
