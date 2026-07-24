<script>
  import TerminalConsole from './TerminalConsole.svelte';
</script>

<main>
  <h1>ZenFabrique | Control Room</h1>
  <p class="subtitle">Live tail of the orchestrator's Observe -&gt; Reason -&gt; Act loop.</p>
  <TerminalConsole />
</main>

<style>
  main {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
    max-width: 960px;
    margin: 2rem auto;
    padding: 0 1rem;
    color: #e2e8f0;
  }

  h1 {
    margin-bottom: 0.25rem;
  }

  .subtitle {
    color: #94a3b8;
    margin-top: 0;
    margin-bottom: 1.5rem;
  }

  :global(body) {
    background: #0f172a;
    margin: 0;
  }
</style>
