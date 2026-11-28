import { DOCUMENT } from '@angular/common';
import { TestBed } from '@angular/core/testing';
import { VisiblePoller } from './visible-poller';

describe('VisiblePoller', () => {
  let visibility: DocumentVisibilityState;
  let online: boolean;
  let task: ReturnType<typeof vi.fn<() => void>>;
  let poller: VisiblePoller;
  let document: Document;

  beforeEach(() => {
    vi.useFakeTimers();
    visibility = 'visible';
    online = true;
    task = vi.fn<() => void>();
    TestBed.configureTestingModule({});
    document = TestBed.inject(DOCUMENT);
    vi.spyOn(document, 'visibilityState', 'get').mockImplementation(() => visibility);
    vi.spyOn(window.navigator, 'onLine', 'get').mockImplementation(() => online);
    poller = TestBed.inject(VisiblePoller);
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  function setVisibility(state: DocumentVisibilityState) {
    visibility = state;
    document.dispatchEvent(new Event('visibilitychange'));
  }

  it('runs the task every interval while visible', () => {
    const handle = poller.start(task, 15_000);
    vi.advanceTimersByTime(14_999);
    expect(task).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(task).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(30_000);
    expect(task).toHaveBeenCalledTimes(3);
    handle.stop();
  });

  it('pauses while hidden and refreshes immediately when visible again', () => {
    const handle = poller.start(task, 15_000);
    setVisibility('hidden');
    vi.advanceTimersByTime(60_000);
    expect(task).not.toHaveBeenCalled();
    setVisibility('visible');
    expect(task).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(15_000);
    expect(task).toHaveBeenCalledTimes(2);
    handle.stop();
  });

  it('does not start polling in a background tab', () => {
    visibility = 'hidden';
    const handle = poller.start(task, 15_000);
    vi.advanceTimersByTime(60_000);
    expect(task).not.toHaveBeenCalled();
    handle.stop();
  });

  it('refreshes on focus and when the connection returns, but not while offline', () => {
    const handle = poller.start(task, 15_000);
    window.dispatchEvent(new Event('focus'));
    expect(task).toHaveBeenCalledTimes(1);
    online = false;
    vi.advanceTimersByTime(15_000);
    expect(task).toHaveBeenCalledTimes(1);
    online = true;
    window.dispatchEvent(new Event('online'));
    expect(task).toHaveBeenCalledTimes(2);
    handle.stop();
  });

  it('stops all timers and listeners', () => {
    const handle = poller.start(task, 15_000);
    handle.stop();
    vi.advanceTimersByTime(60_000);
    window.dispatchEvent(new Event('focus'));
    setVisibility('visible');
    expect(task).not.toHaveBeenCalled();
  });
});
