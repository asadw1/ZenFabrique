import { render, screen, cleanup } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import TerminalConsole from './TerminalConsole.svelte';

// A hand-rolled WebSocket double rather than a mocking library: the
// component only ever calls addEventListener/close, so this is a handful
// of lines and keeps the test's assumptions about the browser API explicit.
class MockWebSocket {
  static instances = [];

  constructor(url) {
    this.url = url;
    this.listeners = {};
    MockWebSocket.instances.push(this);
  }

  addEventListener(type, cb) {
    (this.listeners[type] ??= []).push(cb);
  }

  close() {
    this.emit('close', {});
  }

  emit(type, event) {
    for (const cb of this.listeners[type] ?? []) cb(event);
  }
}

beforeEach(() => {
  MockWebSocket.instances = [];
  vi.stubGlobal('WebSocket', MockWebSocket);
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

function latestSocket() {
  return MockWebSocket.instances[MockWebSocket.instances.length - 1];
}

describe('TerminalConsole', () => {
  it('renders an incoming telemetry line with its level and fields', async () => {
    render(TerminalConsole, { props: { wsUrl: 'ws://test' } });
    const ws = latestSocket();
    ws.emit('open', {});
    ws.emit('message', {
      data: JSON.stringify({
        ts_ms: 0,
        level: 'WARN',
        target: 'zenfabrique_orchestrator::validate',
        message: 'schema breach detected — attempting self-healing repair',
        fields: { source: '"e1.json"' },
      }),
    });

    expect(await screen.findByText('schema breach detected — attempting self-healing repair')).toBeTruthy();
    expect(screen.getByText('WARN')).toBeTruthy();
    expect(screen.getByText('source="e1.json"')).toBeTruthy();
  });

  it('shows connection status transitions', async () => {
    render(TerminalConsole, { props: { wsUrl: 'ws://test' } });
    const ws = latestSocket();
    expect(screen.getByText('connecting')).toBeTruthy();

    ws.emit('open', {});
    expect(await screen.findByText('connected')).toBeTruthy();

    ws.emit('close', {});
    expect(await screen.findByText('disconnected')).toBeTruthy();
  });

  it('shows a raw fallback line for telemetry data that fails to parse, instead of dropping it silently', async () => {
    render(TerminalConsole, { props: { wsUrl: 'ws://test' } });
    const ws = latestSocket();
    ws.emit('message', { data: 'not valid json' });

    expect(await screen.findByText(/failed to parse telemetry line: not valid json/)).toBeTruthy();
  });

  it('caps the number of rendered lines at maxLines, keeping the most recent', async () => {
    render(TerminalConsole, { props: { wsUrl: 'ws://test', maxLines: 3 } });
    const ws = latestSocket();
    for (let i = 0; i < 5; i++) {
      ws.emit('message', {
        data: JSON.stringify({ ts_ms: i, level: 'INFO', target: 't', message: `line ${i}`, fields: {} }),
      });
    }

    expect(screen.queryByText('line 0')).toBeNull();
    expect(screen.queryByText('line 1')).toBeNull();
    expect(await screen.findByText('line 4')).toBeTruthy();
  });

  it('reconnects after the socket closes', async () => {
    vi.useFakeTimers();
    render(TerminalConsole, { props: { wsUrl: 'ws://test' } });
    expect(MockWebSocket.instances.length).toBe(1);

    latestSocket().emit('close', {});
    await vi.advanceTimersByTimeAsync(2100);

    expect(MockWebSocket.instances.length).toBe(2);
  });
});
