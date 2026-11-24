import { ChangeDetectionStrategy, Component } from '@angular/core';

@Component({
  selector: 'einkaufio-root',
  standalone: true,
  templateUrl: './app.html',
  styleUrl: './app.css',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class App {
  readonly featureHighlights = [
    'Listen gemeinsam bearbeiten',
    'Änderungen synchron halten',
    'Mit Familie und Freunden teilen',
  ];
}
