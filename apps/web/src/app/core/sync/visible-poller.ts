import { DOCUMENT } from '@angular/common';
import { inject, Injectable } from '@angular/core';

export const LIST_POLL_INTERVAL_MS = 15_000;

export interface PollerHandle {
  stop(): void;
}

/**
 * Runs `task` every `intervalMs` while the document is visible and the browser is online
 * (ADR 004). The timer is cleared while hidden; becoming visible, regaining focus or coming
 * back online runs the task immediately. Callers must tolerate overlapping triggers.
 */
@Injectable({ providedIn: 'root' })
export class VisiblePoller {
  private readonly document = inject(DOCUMENT);

  start(task: () => void, intervalMs: number = LIST_POLL_INTERVAL_MS): PollerHandle {
    const view = this.document.defaultView;
    let timer: ReturnType<typeof setInterval> | null = null;

    const isVisible = () => this.document.visibilityState === 'visible';
    const isOnline = () => view?.navigator.onLine !== false;
    const runIfActive = () => {
      if (isVisible() && isOnline()) {
        task();
      }
    };
    const arm = () => {
      if (timer === null && isVisible()) {
        timer = setInterval(runIfActive, intervalMs);
      }
    };
    const disarm = () => {
      if (timer !== null) {
        clearInterval(timer);
        timer = null;
      }
    };
    const onVisibilityChange = () => {
      if (isVisible()) {
        runIfActive();
        arm();
      } else {
        disarm();
      }
    };

    this.document.addEventListener('visibilitychange', onVisibilityChange);
    view?.addEventListener('focus', runIfActive);
    view?.addEventListener('online', runIfActive);
    arm();

    return {
      stop: () => {
        disarm();
        this.document.removeEventListener('visibilitychange', onVisibilityChange);
        view?.removeEventListener('focus', runIfActive);
        view?.removeEventListener('online', runIfActive);
      },
    };
  }
}
