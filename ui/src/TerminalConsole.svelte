<script>
  // A live tail of the orchestrator's own tracing output (README's
  // "Observability Stream") — one line per WebSocket message, already
  // structured JSON from telemetry.rs's BroadcastLayer, so no parsing
  // beyond JSON.parse is needed here.
  let { wsUrl = 'ws://localhost:9001', maxLines = 500 } = $props();

  let lines = $state([]);
  let status = $state('connecting');
  let bodyEl;

  let socket;
  let reconnectTimer;

  function levelClass(level) {
    switch (level) {
      case 'ERROR':
        return 'level-error';
      case 'WARN':
        return 'level-warn';
      case 'INFO':
        return 'level-info';
      default:
        return 'level-debug';
    }
  }

  function formatTime(tsMs) {
    return new Date(tsMs).toLocaleTimeString();
  }

  function pushLine(entry) {
    lines = [...lines, entry].slice(-maxLines);
  }

  function connect() {
    socket = new WebSocket(wsUrl);
    status = 'connecting';

    socket.addEventListener('open', () => {
      status = 'connected';
    });

    socket.addEventListener('message', (event) => {
      try {
        pushLine(JSON.parse(event.data));
      } catch {
        // A malformed line is itself diagnostic information — show it
        // rather than silently dropping it, so a wire-format bug is
        // visible in the console instead of just missing entries.
        pushLine({
          ts_ms: Date.now(),
          level: 'ERROR',
          target: 'console',
          message: `failed to parse telemetry line: ${event.data}`,
          fields: {},
        });
      }
    });

    socket.addEventListener('close', () => {
      status = 'disconnected';
      reconnectTimer = setTimeout(connect, 2000);
    });

    socket.addEventListener('error', () => {
      status = 'error';
    });
  }

  $effect(() => {
    connect();
    return () => {
      clearTimeout(reconnectTimer);
      socket?.close();
    };
  });

  $effect(() => {
    if (lines.length && bodyEl) {
      bodyEl.scrollTop = bodyEl.scrollHeight;
    }
  });
</script>

<div class="console">
  <div class="console-header">
    <span class="status status-{status}">{status}</span>
    <span class="title">zenfabrique-orchestrator</span>
  </div>
  <div class="console-body" bind:this={bodyEl}>
    {#each lines as line, i (i)}
      <div class="line {levelClass(line.level)}">
        <span class="ts">{formatTime(line.ts_ms)}</span>
        <span class="level">{line.level}</span>
        <span class="target">{line.target}</span>
        <span class="message">{line.message}</span>
        {#if line.fields && Object.keys(line.fields).length > 0}
          <span class="fields">{Object.entries(line.fields).map(([k, v]) => `${k}=${v}`).join(' ')}</span>
        {/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .console {
    font-family: 'Cascadia Code', 'Fira Code', Consolas, monospace;
    background: #05070d;
    border: 1px solid #1e293b;
    border-radius: 6px;
    overflow: hidden;
  }

  .console-header {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.5rem 0.75rem;
    background: #111827;
    border-bottom: 1px solid #1e293b;
    font-size: 0.85rem;
  }

  .status {
    padding: 0.1rem 0.5rem;
    border-radius: 4px;
    font-weight: 600;
    text-transform: uppercase;
    font-size: 0.7rem;
  }

  .status-connected {
    background: #14532d;
    color: #86efac;
  }

  .status-connecting {
    background: #713f12;
    color: #fde68a;
  }

  .status-disconnected,
  .status-error {
    background: #7f1d1d;
    color: #fca5a5;
  }

  .console-body {
    height: 420px;
    overflow-y: auto;
    padding: 0.5rem 0.75rem;
    font-size: 0.8rem;
    line-height: 1.5;
  }

  .line {
    white-space: pre-wrap;
    word-break: break-word;
  }

  .ts {
    color: #64748b;
    margin-right: 0.5rem;
  }

  .level {
    display: inline-block;
    min-width: 3.5rem;
    margin-right: 0.5rem;
    font-weight: 700;
  }

  .level-error .level {
    color: #f87171;
  }

  .level-warn .level {
    color: #fbbf24;
  }

  .level-info .level {
    color: #4ade80;
  }

  .level-debug .level {
    color: #94a3b8;
  }

  .target {
    color: #64748b;
    margin-right: 0.5rem;
  }

  .message {
    color: #e2e8f0;
  }

  .fields {
    color: #64748b;
    margin-left: 0.5rem;
  }
</style>
