<script>
  // Pavé numérique : gros boutons, utilisable avec des gants ou sur une tablette murale.
  import Sheet from './Sheet.svelte';

  let { title = 'Code', hint = '', error = '', onsubmit, onclose } = $props();
  let code = $state('');

  const KEYS = ['1', '2', '3', '4', '5', '6', '7', '8', '9', '', '0', '⌫'];

  function press(k) {
    if (k === '⌫') code = code.slice(0, -1);
    else if (k && code.length < 12) code += k;
  }

  function submit() {
    if (code.length >= 4) {
      onsubmit(code);
      code = '';
    }
  }

  function onkeydown(e) {
    if (/^[0-9]$/.test(e.key)) press(e.key);
    else if (e.key === 'Backspace') press('⌫');
    else if (e.key === 'Enter') submit();
    else return;
    e.preventDefault();
  }
</script>

<svelte:window {onkeydown} />

<Sheet {title} {onclose}>
  {#if hint}<p class="hint">{hint}</p>{/if}
  <div class="dots" aria-label="{code.length} chiffres saisis" class:shake={!!error}>
    {#each Array(Math.max(4, code.length)) as _, i (i)}
      <span class:on={i < code.length}></span>
    {/each}
  </div>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  <div class="pad">
    {#each KEYS as k, i (i)}
      {#if k}
        <button type="button" class="key" class:back={k === '⌫'} aria-label={k === '⌫' ? 'Effacer' : k} onclick={() => press(k)}>{k}</button>
      {:else}
        <span></span>
      {/if}
    {/each}
  </div>
  <button class="btn ok" disabled={code.length < 4} onclick={submit}>Valider</button>
</Sheet>

<style>
  .hint {
    margin: 0 0 12px;
    color: var(--muted);
  }

  .dots {
    display: flex;
    justify-content: center;
    gap: 14px;
    margin: 8px 0 18px;
  }

  .dots span {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 2px solid var(--muted);
  }

  .dots span.on {
    background: var(--ink);
    border-color: var(--ink);
  }

  .shake {
    animation: shake 0.3s;
  }

  @keyframes shake {
    25% {
      transform: translateX(-6px);
    }
    75% {
      transform: translateX(6px);
    }
  }

  .error {
    text-align: center;
    color: var(--alert);
    font-weight: 600;
    margin: -6px 0 14px;
  }

  .pad {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 10px;
    max-width: 320px;
    margin: 0 auto 18px;
  }

  .key {
    height: 64px;
    border-radius: 18px;
    background: var(--surface-2);
    font-size: 26px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  .key:active {
    background: var(--line);
  }

  .key.back {
    font-size: 22px;
    background: transparent;
  }

  .ok {
    display: flex;
    justify-content: center;
    width: 100%;
    max-width: 320px;
    margin: 0 auto;
  }
</style>
