import { provideHttpClient, withInterceptors } from '@angular/common/http';
import { ApplicationConfig, provideZonelessChangeDetection } from '@angular/core';
import { provideRouter, withComponentInputBinding } from '@angular/router';
import { routes } from './app.routes';
import { provideEinkaufioAuth } from './core/auth/auth-config';
import { authInterceptor } from './core/auth/auth.interceptor';
import { RUNTIME_CONFIG, RuntimeConfig } from './core/config/runtime-config';

/** Composition root: everything deployment-specific comes from the validated runtime config. */
export function createAppConfig(runtimeConfig: RuntimeConfig, origin: string): ApplicationConfig {
  return {
    providers: [
      provideZonelessChangeDetection(),
      provideRouter(routes, withComponentInputBinding()),
      provideHttpClient(withInterceptors([authInterceptor])),
      { provide: RUNTIME_CONFIG, useValue: runtimeConfig },
      provideEinkaufioAuth(runtimeConfig, origin),
    ],
  };
}
