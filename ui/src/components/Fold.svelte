<script>
  // Un bloc repliable : titre, petite ligne d'état, chevron. C'est l'utilisateur qui l'ouvre
  // ou le ferme ; le parent peut aussi l'ouvrir (bind:shown, ex. l'étape suivante). Ne jamais
  // lui passer une valeur recalculée à chaque rafraîchissement : il se refermerait tout seul.
  // `tone` colore la pastille : ok (vert), bad (rouge), wait (orange).
  import Icon from './Icon.svelte';

  let { title, sub = '', icon = null, tone = '', shown = $bindable(false), step = null, children } = $props();
</script>

<section class="fold {tone}" class:shown>
  <button class="head" type="button" aria-expanded={shown} onclick={() => (shown = !shown)}>
    {#if step != null}
      <span class="mark">{#if tone === 'ok'}<Icon name="check" size={16} />{:else if tone === 'bad'}<Icon name="close" size={16} />{:else}{step}{/if}</span>
    {:else if icon}
      <span class="mark"><Icon name={icon} size={18} /></span>
    {/if}
    <span class="txt">
      <strong>{title}</strong>
      {#if sub}<small>{sub}</small>{/if}
    </span>
    <span class="chev" aria-hidden="true"><Icon name="chevron" size={18} /></span>
  </button>
  {#if shown}
    <div class="body">{@render children?.()}</div>
  {/if}
</section>

<style>
  .fold {
    margin: 10px 0;
    border: 1px solid var(--line);
    border-radius: 16px;
    background: var(--surface);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    min-height: 56px;
    padding: 10px 14px;
    text-align: left;
    color: var(--ink);
  }

  .mark {
    flex: none;
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    background: var(--surface-2);
    font-weight: 700;
    font-size: 14px;
  }

  .ok .mark {
    background: var(--moss);
    color: var(--surface);
  }

  .bad .mark {
    background: var(--alert);
    color: var(--surface);
  }

  .wait .mark {
    box-shadow: inset 0 0 0 2px #d9a441;
  }

  .txt {
    flex: 1;
    display: grid;
    gap: 2px;
    min-width: 0;
  }

  .txt strong {
    font-weight: 600;
  }

  .txt small {
    color: var(--muted);
    font-size: 13px;
  }

  .ok .txt small {
    color: var(--moss);
  }

  .bad .txt small {
    color: var(--alert);
  }

  .chev {
    flex: none;
    color: var(--muted);
    transition: transform 0.15s;
  }

  .shown .chev {
    transform: rotate(90deg);
  }

  .body {
    display: grid;
    gap: 10px;
    padding: 0 14px 14px;
  }
</style>
