import { bootstrapApplication } from '@angular/platform-browser';
import { App } from './app';
import { createAppConfig } from './app.config';
import { loadRuntimeConfig, RuntimeConfig } from './core/config/runtime-config';

/** Loads `/config.json`, then starts Angular; without valid configuration the app does not start. */
export async function bootstrap(document: Document): Promise<void> {
  let runtimeConfig: RuntimeConfig;
  try {
    runtimeConfig = await loadRuntimeConfig();
  } catch (error) {
    console.error('Einkaufio: Laufzeitkonfiguration ungültig oder nicht erreichbar.', error instanceof Error ? error.message : '');
    renderStartupError(document);
    return;
  }
  await bootstrapApplication(App, createAppConfig(runtimeConfig, document.location.origin));
}

function renderStartupError(document: Document): void {
  const container = document.createElement('main');
  container.setAttribute('role', 'alert');
  container.className = 'startup-error';
  const title = document.createElement('h1');
  title.textContent = 'Einkaufio ist gerade nicht verfügbar';
  const text = document.createElement('p');
  text.textContent = 'Die App konnte nicht gestartet werden. Bitte lade die Seite später erneut.';
  container.append(title, text);
  document.body.replaceChildren(container);
}
