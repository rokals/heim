<script>
  import { tr } from '../lib/i18n.js';
  // Panneau qui monte du bas de l'écran (ou fenêtre centrée sur ordinateur).
  import Icon from './Icon.svelte';

  let { title, onclose, children } = $props();
  let dialog;
  // Un panneau qui en remplace un autre (« Ajouter » → réglages du bouton) : la fermeture
  // de l'ancien ne doit pas fermer le nouveau.
  let gone = false;

  $effect(() => {
    dialog.showModal();
    return () => {
      gone = true;
      if (dialog.open) dialog.close();
    };
  });
</script>

<dialog
  bind:this={dialog}
  onclose={() => !gone && onclose?.()}
  onclick={(e) => e.target === dialog && dialog.close()}
  aria-labelledby="sheet-title"
>
  <div class="panel">
    <header>
      <h2 id="sheet-title">{title}</h2>
      <button class="close" onclick={() => dialog.close()} aria-label={tr("Fermer")}><Icon name="close" /></button>
    </header>
    {@render children()}
  </div>
</dialog>

<style>
  dialog {
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--ink);
    width: 100%;
    max-width: 560px;
    max-height: 92vh;
    margin: auto auto 0;
  }

  dialog::backdrop {
    background: rgba(12, 20, 24, 0.45);
  }

  .panel {
    background: var(--bg);
    border-radius: 24px 24px 0 0;
    padding: 8px 20px calc(24px + env(safe-area-inset-bottom));
    max-height: 92vh;
    overflow-y: auto;
    animation: rise 0.22s ease-out;
  }

  @media (min-width: 720px) {
    dialog {
      margin: auto;
    }
    .panel {
      border-radius: 24px;
    }
  }

  @keyframes rise {
    from {
      transform: translateY(24px);
      opacity: 0;
    }
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 12px 0 16px;
  }

  h2 {
    margin: 0;
    font-size: 22px;
    line-height: 1.2;
    overflow-wrap: anywhere;
  }

  .close {
    flex: none;
    width: 44px;
    height: 44px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: var(--surface-2);
  }
</style>
