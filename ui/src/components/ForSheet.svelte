<script>
  // « Allumer pour… » : une durée, et l'appareil s'éteint tout seul à la fin.
  import Sheet from './Sheet.svelte';
  import { turnOnFor } from '../lib/store.svelte.js';

  let { entity, onclose } = $props();
  let custom = $state(20);

  const CHOICES = [5, 15, 30, 60, 120];

  function go(minutes) {
    turnOnFor(entity.entity_id, minutes);
    onclose();
  }
</script>

<Sheet title="Allumer {entity.name} pour…" {onclose}>
  <div class="choices">
    {#each CHOICES as m (m)}
      <button class="choice" onclick={() => go(m)}>{m < 60 ? `${m} min` : `${m / 60} h`}</button>
    {/each}
  </div>
  <form
    class="custom"
    onsubmit={(e) => {
      e.preventDefault();
      if (custom > 0) go(custom);
    }}
  >
    <input class="input num" type="number" min="1" max="10080" bind:value={custom} aria-label="Durée en minutes" />
    <span>minutes</span>
    <button class="btn" type="submit">Allumer</button>
  </form>
  <p class="note">S'il est éteint à la main avant la fin, la minuterie s'annule.</p>
</Sheet>

<style>
  .choices {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(96px, 1fr));
    gap: 8px;
  }

  .choice {
    min-height: 56px;
    border-radius: 16px;
    background: var(--surface);
    box-shadow: var(--shadow);
    font-size: 18px;
    font-weight: 600;
  }

  .custom {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 18px;
    color: var(--muted);
  }

  .custom .input {
    width: 100px;
  }

  .note {
    font-size: 14px;
    color: var(--muted);
  }
</style>
