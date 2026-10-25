<script>
  // Connexion : on touche son prénom, puis son mot de passe. Pas d'identifiant à retenir.
  import { home, login } from '../lib/store.svelte.js';

  let chosen = $state(home.loginUsers.length === 1 ? home.loginUsers[0] : null);
  let password = $state('');
  let error = $state('');
  let busy = $state(false);
  let input = $state();

  $effect(() => {
    if (chosen) input?.focus();
  });

  async function submit(e) {
    e.preventDefault();
    busy = true;
    error = '';
    try {
      await login(chosen.id, password);
    } catch (err) {
      error = err.message;
      password = '';
    } finally {
      busy = false;
    }
  }
</script>

<section class="login">
  <svg viewBox="0 0 32 32" width="56" height="56" aria-hidden="true">
    <path d="M6 15 16 7l10 8v11H6z" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linejoin="round" />
    <path d="M13 26v-6h6v6" fill="none" stroke="var(--ember)" stroke-width="2.4" stroke-linejoin="round" />
  </svg>
  <h1>Qui est-ce ?</h1>

  {#if !chosen}
    <ul class="people">
      {#each home.loginUsers as u (u.id)}
        <li>
          <button onclick={() => (chosen = u)}>
            <span class="avatar" aria-hidden="true">{u.name.slice(0, 1).toUpperCase()}</span>
            {u.name}
          </button>
        </li>
      {/each}
    </ul>
  {:else}
    <form onsubmit={submit}>
      <p class="who">
        <span class="avatar" aria-hidden="true">{chosen.name.slice(0, 1).toUpperCase()}</span>
        <strong>{chosen.name}</strong>
        {#if home.loginUsers.length > 1}
          <button type="button" class="link" onclick={() => ((chosen = null), (error = ''))}>Changer</button>
        {/if}
      </p>
      <label class="field">
        <span>Mot de passe</span>
        <input class="input" type="password" bind:this={input} bind:value={password} autocomplete="current-password" />
      </label>
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      <button class="btn wide" type="submit" disabled={busy || !password}>Entrer</button>
    </form>
  {/if}
</section>

<style>
  .login {
    max-width: 380px;
    margin: 8vh auto 0;
    text-align: center;
  }

  h1 {
    font-size: 34px;
    margin: 12px 0 24px;
  }

  .people {
    list-style: none;
    padding: 0;
    margin: 0;
    display: grid;
    gap: 10px;
  }

  .people button {
    display: flex;
    align-items: center;
    gap: 14px;
    width: 100%;
    min-height: 64px;
    padding: 10px 16px;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
    font-size: 19px;
    font-weight: 600;
    text-align: left;
  }

  .avatar {
    display: inline-grid;
    place-items: center;
    flex: none;
    width: 42px;
    height: 42px;
    border-radius: 50%;
    background: var(--ink);
    color: var(--bg);
    font-weight: 600;
  }

  form {
    text-align: left;
  }

  .who {
    display: flex;
    align-items: center;
    gap: 12px;
    font-size: 19px;
    margin: 0 0 18px;
  }

  .link {
    margin-left: auto;
    color: var(--muted);
    font-weight: 600;
  }

  .error {
    color: var(--alert);
    font-weight: 600;
    margin: -6px 0 14px;
  }

  .wide {
    width: 100%;
    justify-content: center;
  }
</style>
