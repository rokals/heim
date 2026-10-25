<script>
  import Icon from './components/Icon.svelte';
  import Home from './components/Home.svelte';
  import Scenes from './components/Scenes.svelte';
  import Devices from './components/Devices.svelte';
  import Automations from './components/Automations.svelte';
  import Settings from './components/Settings.svelte';
  import LoginScreen from './components/LoginScreen.svelte';
  import SetupWizard from './components/SetupWizard.svelte';
  import { connect, home, isMember, setToken } from './lib/store.svelte.js';

  const TABS = [
    { id: 'maison', label: 'Accueil', icon: 'house' },
    { id: 'appareils', label: 'Appareils', icon: 'grid' },
    { id: 'scenes', label: 'Scènes', icon: 'scene' },
    { id: 'automatismes', label: 'Automatismes', icon: 'flow' },
    { id: 'reglages', label: 'Réglages', icon: 'sliders' },
  ];

  // Adresse du type #reglages/connexions : l'onglet, puis la page dans l'onglet.
  const fromHash = () => {
    const [t = '', ...rest] = location.hash.slice(1).split('/');
    const sub = rest.join('/');
    return { tab: TABS.some((x) => x.id === t) ? t : 'maison', sub };
  };
  let route = $state(fromHash());
  const tab = $derived(route.tab);
  let code = $state('');

  // Connexion perdue : après 1,5 s (pas de clignotement pour une micro-coupure), la page se
  // grise et ne prend plus les clics, jusqu'au retour de Heim.
  let lostAt = $state(null);
  $effect(() => {
    if (home.connected || !home.loaded || home.needLogin || home.needToken) lostAt = null;
    else if (lostAt === null) lostAt = Date.now();
  });
  const offline = $derived(lostAt !== null && home.now - lostAt > 1500);

  $effect(() => {
    const onHash = () => (route = fromHash());
    // Le bouton « retour » du téléphone ramène à la page précédente.
    addEventListener('popstate', onHash);
    addEventListener('hashchange', onHash);
    return () => {
      removeEventListener('popstate', onHash);
      removeEventListener('hashchange', onHash);
    };
  });

  function go(id, sub = '') {
    const hash = id === 'maison' ? '' : sub ? `${id}/${sub}` : id;
    history.pushState(null, '', hash ? `#${hash}` : location.pathname + location.search);
    route = { tab: id, sub };
    scrollTo({ top: 0 });
  }
</script>

<div class="shell">
  <header class="bar">
    <a class="mark" href="#" onclick={(e) => { e.preventDefault(); go('maison'); }} aria-label="Heim, accueil">
      <svg viewBox="0 0 32 32" width="30" height="30" aria-hidden="true">
        <path d="M6 15 16 7l10 8v11H6z" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linejoin="round" />
        <path d="M13 26v-6h6v6" fill="none" stroke="var(--ember)" stroke-width="2.4" stroke-linejoin="round" />
      </svg>
      <span>Heim</span>
    </a>
    {#if !home.needLogin && !home.needToken && !home.setup?.needed}
    <nav class="tabs" aria-label="Sections">
      {#each TABS.filter((t) => t.id !== 'automatismes' || isMember()) as t (t.id)}
        <button class="tab" class:current={tab === t.id} aria-current={tab === t.id ? 'page' : undefined} onclick={() => go(t.id)}>
          <Icon name={t.icon} size={22} />
          <span>{t.label}</span>
        </button>
      {/each}
    </nav>
    {/if}
  </header>

  <main>
    {#if home.setup?.needed && (!home.setup.accounts || home.me?.user)}
      <SetupWizard />
    {:else if home.needLogin}
      <LoginScreen />
    {:else if home.needToken}
      <section class="gate">
        <h1>Code d'accès</h1>
        <p>Ce Heim est protégé. Entrez le code défini au démarrage (HEIM_TOKEN).</p>
        <form
          onsubmit={(e) => {
            e.preventDefault();
            setToken(code);
          }}
        >
          <input class="input" type="password" bind:value={code} autocomplete="current-password" aria-label="Code d'accès" />
          <button class="btn" type="submit" disabled={!code.trim()}>Entrer</button>
        </form>
      </section>
    {:else if tab === 'scenes'}
      <Scenes />
    {:else if tab === 'appareils'}
      <Devices />
    {:else if tab === 'automatismes'}
      <Automations page={route.sub} onnavigate={(sub) => go('automatismes', sub)} />
    {:else if tab === 'reglages'}
      <Settings page={route.sub} onnavigate={(sub) => go('reglages', sub)} />
    {:else}
      <Home onnavigate={(sub) => go('automatismes', sub)} />
    {/if}
  </main>
</div>

{#if offline}
  <div class="veil" role="alertdialog" aria-live="assertive" aria-label="Connexion perdue">
    <div class="lost">
      <span class="dot" aria-hidden="true"></span>
      <strong>Connexion à Heim perdue</strong>
      <span>Reconnexion en cours… Les boutons sont en pause pour ne rien envoyer dans le vide.</span>
      <button class="btn quiet" onclick={() => connect()}>Réessayer maintenant</button>
    </div>
  </div>
{/if}

<div class="toasts" aria-live="polite">
  {#each home.toasts as t (t.id)}
    <div class="toast" class:alert={t.kind === 'alert'}>{t.text}</div>
  {/each}
</div>

<style>
  .veil {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    padding: 24px;
    background: color-mix(in srgb, var(--bg) 55%, transparent);
    backdrop-filter: grayscale(0.9) blur(1.5px);
    -webkit-backdrop-filter: grayscale(0.9) blur(1.5px);
    animation: fade 0.3s ease;
  }

  .lost {
    display: grid;
    justify-items: center;
    gap: 8px;
    max-width: 340px;
    padding: 22px 24px;
    border-radius: 22px;
    background: var(--surface);
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.18);
    text-align: center;
  }

  .lost span {
    color: var(--muted);
    font-size: 15px;
  }

  .dot {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--ember);
    animation: pulse 1.2s ease-in-out infinite;
  }

  @keyframes pulse {
    50% {
      transform: scale(0.6);
      opacity: 0.4;
    }
  }

  @keyframes fade {
    from {
      opacity: 0;
    }
  }

  .shell {
    max-width: 1080px;
    margin: 0 auto;
    padding: 0 16px calc(96px + env(safe-area-inset-bottom));
  }

  .bar {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: calc(14px + env(safe-area-inset-top)) 0 18px;
  }

  .mark {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    color: var(--ink);
    text-decoration: none;
    font-weight: 600;
    font-size: 21px;
    letter-spacing: 0.01em;
  }

  /* Téléphone : la navigation est un pouce en bas de l'écran. */
  .tabs {
    position: fixed;
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 10;
    display: grid;
    grid-template-columns: repeat(5, 1fr);
    padding: 6px 8px calc(6px + env(safe-area-inset-bottom));
    background: color-mix(in srgb, var(--bg) 88%, transparent);
    backdrop-filter: blur(14px);
    border-top: 1px solid var(--line);
  }

  .tab {
    display: grid;
    justify-items: center;
    gap: 2px;
    min-height: 54px;
    padding-top: 6px;
    font-size: 11px;
    color: var(--muted);
    border-radius: 14px;
    min-width: 0;
  }

  .tab span {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tab.current {
    color: var(--ink);
    font-weight: 600;
  }

  .tab.current :global(svg) {
    color: var(--ember);
  }

  /* Ordinateur : les onglets remontent dans la barre du haut. */
  @media (min-width: 760px) {
    .shell {
      padding: 0 32px 64px;
    }

    .bar {
      padding: 22px 0 28px;
    }

    .tabs {
      position: static;
      display: flex;
      gap: 4px;
      padding: 0;
      background: none;
      backdrop-filter: none;
      border: 0;
      margin-left: auto;
    }

    .tab {
      display: inline-flex;
      align-items: center;
      gap: 8px;
      min-height: 44px;
      padding: 0 16px;
      font-size: 15px;
    }

    .tab.current {
      background: var(--surface);
      box-shadow: var(--shadow);
    }
  }

  .gate {
    max-width: 420px;
    padding-top: 10vh;
  }

  .gate h1 {
    font-size: 40px;
    margin: 0 0 8px;
  }

  .gate p {
    color: var(--muted);
  }

  .gate form {
    display: flex;
    gap: 10px;
  }

  .toasts {
    position: fixed;
    left: 16px;
    right: 16px;
    bottom: calc(84px + env(safe-area-inset-bottom));
    z-index: 30;
    display: grid;
    gap: 8px;
    justify-items: center;
    pointer-events: none;
  }

  @media (min-width: 760px) {
    .toasts {
      bottom: 24px;
    }
  }

  .toast {
    max-width: 480px;
    padding: 12px 18px;
    border-radius: 16px;
    background: var(--ink);
    color: var(--bg);
    box-shadow: 0 8px 24px rgba(12, 20, 24, 0.25);
    animation: pop 0.2s ease-out;
  }

  .toast.alert {
    background: var(--alert);
    color: #fff;
  }

  @keyframes pop {
    from {
      transform: translateY(8px);
      opacity: 0;
    }
  }
</style>
