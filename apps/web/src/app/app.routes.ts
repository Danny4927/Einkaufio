import { Routes } from '@angular/router';
import { authGuard } from './core/auth/auth.guard';

export const routes: Routes = [
  {
    path: '',
    pathMatch: 'full',
    title: 'Einkaufio – Gemeinsam einkaufen',
    loadComponent: () => import('./features/home/home-page').then((m) => m.HomePage),
  },
  {
    path: 'auth/callback',
    title: 'Anmeldung – Einkaufio',
    loadComponent: () => import('./features/auth/auth-callback-page').then((m) => m.AuthCallbackPage),
  },
  {
    path: 'listen',
    canActivate: [authGuard],
    title: 'Meine Listen – Einkaufio',
    loadComponent: () => import('./features/lists/list-overview-page').then((m) => m.ListOverviewPage),
  },
  {
    path: 'listen/:listId',
    canActivate: [authGuard],
    title: 'Liste – Einkaufio',
    loadComponent: () => import('./features/lists/list-editor-page').then((m) => m.ListEditorPage),
  },
  { path: '**', redirectTo: '' },
];
