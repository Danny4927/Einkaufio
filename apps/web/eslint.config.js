// @ts-check
const eslint = require('@eslint/js');
const tseslint = require('typescript-eslint');
const angular = require('angular-eslint');
const boundaries = require('eslint-plugin-boundaries');

/**
 * Import boundaries (S24):
 * - core:     app-wide singletons (auth, http, config); may import shared.
 * - shared:   reusable, feature-agnostic UI/utilities; may import only shared.
 * - feature:  src/app/features/<name>; may import core, shared and itself, never another feature.
 * - app:      root shell/composition (app.ts, app.config.ts, app.routes.ts); may import everything.
 */
module.exports = tseslint.config(
  { ignores: ['dist/**', 'node_modules/**', '.angular/**', 'test-results/**', 'playwright-report/**'] },
  {
    files: ['**/*.ts'],
    extends: [
      eslint.configs.recommended,
      ...tseslint.configs.recommended,
      ...tseslint.configs.stylistic,
      ...angular.configs.tsRecommended,
    ],
    processor: angular.processInlineTemplates,
    rules: {
      '@angular-eslint/component-selector': ['error', { type: 'element', prefix: 'einkaufio', style: 'kebab-case' }],
      '@angular-eslint/directive-selector': ['error', { type: 'attribute', prefix: 'einkaufio', style: 'camelCase' }],
      '@angular-eslint/component-max-inline-declarations': ['error', { template: 0, styles: 0, animations: 0 }],
    },
  },
  {
    files: ['src/**/*.ts'],
    plugins: { boundaries },
    settings: {
      'import/resolver': { node: { extensions: ['.ts', '.js'] } },
      'boundaries/include': ['src/**/*.ts'],
      'boundaries/elements': [
        { type: 'core', pattern: 'src/app/core' },
        { type: 'shared', pattern: 'src/app/shared' },
        { type: 'feature', pattern: 'src/app/features/*', capture: ['feature'] },
        // First match wins: anything else directly under src/app is the root shell.
        { type: 'app', pattern: 'src/app' },
        { type: 'entry', pattern: 'src' },
      ],
    },
    rules: {
      'boundaries/no-unknown-files': 'error',
      'boundaries/dependencies': [
        'error',
        {
          default: 'disallow',
          policies: [
            { from: { element: { type: 'entry' } }, allow: [{ to: { element: { type: 'app' } } }] },
            { from: { element: { type: 'app' } }, allow: [{ to: { element: { type: 'app' } } }, { to: { element: { type: 'core' } } }, { to: { element: { type: 'shared' } } }, { to: { element: { type: 'feature' } } }] },
            { from: { element: { type: 'core' } }, allow: [{ to: { element: { type: 'core' } } }, { to: { element: { type: 'shared' } } }] },
            { from: { element: { type: 'shared' } }, allow: [{ to: { element: { type: 'shared' } } }] },
            { from: { element: { type: 'feature' } }, allow: [{ to: { element: { type: 'core' } } }, { to: { element: { type: 'shared' } } }, { to: { element: { type: 'feature', captured: { feature: '{{from.captured.feature}}' } } } }] },
          ],
        },
      ],
    },
  },
  {
    files: ['**/*.html'],
    extends: [...angular.configs.templateRecommended, ...angular.configs.templateAccessibility],
  },
);
