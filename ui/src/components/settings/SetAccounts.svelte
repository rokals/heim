<script>
  // Comptes : le sien (mot de passe, déconnexion) et, pour un admin, ceux de la maison.
  import Icon from '../Icon.svelte';
  import { api, home, isAdmin, loadMe, logout, toast } from '../../lib/store.svelte.js';

  const ROLES = [
    { id: 'admin', label: 'Admin', hint: 'Règle tout' },
    { id: 'member', label: 'Membre', hint: "Utilise tout, alarme comprise, sans toucher aux réglages" },
    { id: 'guest', label: 'Invité', hint: 'Lumières, prises et scènes. Pas l’alarme.' },
  ];
  const roleLabel = (r) => ROLES.find((x) => x.id === r)?.label ?? r;

  let users = $state([]);
  let first = $state({ name: '', password: '', confirm: '' });
  let pw = $state({ current: '', next: '' });
  let add = $state({ name: '', role: 'member', password: '' });
  let reset = $state(null);
  let resetPw = $state('');

  const open = $derived(home.me?.open);
  const me = $derived(home.me?.user);

  async function loadUsers() {
    if (!isAdmin() || open) return;
    users = await api('/users').catch(() => []);
  }

  $effect(() => {
    if (home.me) loadUsers();
  });

  async function createFirst(e) {
    e.preventDefault();
    if (first.password !== first.confirm) return toast('Les deux mots de passe ne sont pas identiques', 'alert');
    try {
      await api('/users', { method: 'POST', body: { name: first.name, password: first.password, role: 'admin' } });
      await loadMe();
      toast('Compte créé : Heim est maintenant protégé');
    } catch (err) {
      toast(err.message, 'alert');
    }
  }

  async function changePassword(e) {
    e.preventDefault();
    try {
      await api('/auth/password', { method: 'POST', body: { current: pw.current, new: pw.next } });
      toast('Mot de passe changé : reconnectez-vous');
      pw = { current: '', next: '' };
      await logout();
    } catch (err) {
      toast(err.message, 'alert');
    }
  }

  async function addUser(e) {
    e.preventDefault();
    try {
      await api('/users', { method: 'POST', body: $state.snapshot(add) });
      toast(`Compte « ${add.name} » créé`);
      add = { name: '', role: 'member', password: '' };
      loadUsers();
    } catch (err) {
      toast(err.message, 'alert');
    }
  }

  async function setRole(u, role) {
    try {
      await api(`/users/${u.id}`, { method: 'PUT', body: { role } });
      toast(`${u.name} : ${roleLabel(role)}`);
    } catch (err) {
      toast(err.message, 'alert');
    }
    loadUsers();
  }

  async function saveReset(e) {
    e.preventDefault();
    try {
      await api(`/users/${reset.id}`, { method: 'PUT', body: { password: resetPw } });
      toast(`Nouveau mot de passe pour ${reset.name}`);
      reset = null;
      resetPw = '';
    } catch (err) {
      toast(err.message, 'alert');
    }
  }

  async function remove(u) {
    if (!confirm(`Supprimer le compte de ${u.name} ? Ses appareils seront déconnectés.`)) return;
    try {
      await api(`/users/${u.id}`, { method: 'DELETE' });
      loadUsers();
    } catch (err) {
      toast(err.message, 'alert');
    }
  }

  const fmt = (d) => new Date(d).toLocaleDateString('fr-FR', { day: 'numeric', month: 'short', year: 'numeric' });
</script>

{#if open}
  <section class="card">
    <h2>Protéger Heim</h2>
    <p class="muted">Aujourd'hui, toute personne sur le réseau de la maison peut tout faire. Créez votre compte admin : ensuite, chacun aura le sien (admin, membre ou invité).</p>
    <form onsubmit={createFirst}>
      <label class="field"><span>Votre prénom</span><input class="input" bind:value={first.name} autocomplete="username" /></label>
      <label class="field"><span>Mot de passe (8 caractères ou plus)</span><input class="input" type="password" bind:value={first.password} autocomplete="new-password" /></label>
      <label class="field"><span>Le même, encore</span><input class="input" type="password" bind:value={first.confirm} autocomplete="new-password" /></label>
      <button class="btn" type="submit" disabled={!first.name.trim() || first.password.length < 8}>Créer mon compte admin</button>
    </form>
  </section>
{:else if me}
  <section class="card who">
    <span class="avatar" aria-hidden="true">{me.name.slice(0, 1).toUpperCase()}</span>
    <div class="txt"><strong>{me.name}</strong><span>{roleLabel(me.role)}</span></div>
    <button class="btn quiet" onclick={logout}>Se déconnecter</button>
  </section>

  <h2>Mon mot de passe</h2>
  <form class="pw" onsubmit={changePassword}>
    <input class="input" type="password" placeholder="Mot de passe actuel" bind:value={pw.current} autocomplete="current-password" />
    <input class="input" type="password" placeholder="Nouveau (8 caractères ou plus)" bind:value={pw.next} autocomplete="new-password" />
    <button class="btn quiet" type="submit" disabled={!pw.current || pw.next.length < 8}>Changer</button>
  </form>
  {#if home.me.sessions?.length > 1}
    <p class="muted">Connecté sur {home.me.sessions.length} appareils. Changer de mot de passe les déconnecte tous.</p>
  {/if}
{:else}
  <p class="muted">Heim est protégé par un code d'accès technique (HEIM_TOKEN). Les comptes se créent depuis un navigateur connecté avec ce code.</p>
{/if}

{#if isAdmin() && !open && users.length}
  <h2>Les comptes de la maison</h2>
  <ul class="users">
    {#each users as u (u.id)}
      <li>
        <span class="avatar small" aria-hidden="true">{u.name.slice(0, 1).toUpperCase()}</span>
        <div class="txt"><strong>{u.name}{u.id === me?.id ? ' (vous)' : ''}</strong><span>depuis le {fmt(u.created)}</span></div>
        <select class="input sel" value={u.role} onchange={(e) => setRole(u, e.currentTarget.value)} aria-label="Rôle de {u.name}" disabled={u.id === me?.id}>
          {#each ROLES as r (r.id)}<option value={r.id}>{r.label}</option>{/each}
        </select>
        {#if u.id !== me?.id}
          <button class="mini" aria-label="Nouveau mot de passe pour {u.name}" onclick={() => ((reset = u), (resetPw = ''))}><Icon name="pencil" size={16} /></button>
          <button class="mini" aria-label="Supprimer {u.name}" onclick={() => remove(u)}><Icon name="close" size={16} /></button>
        {/if}
      </li>
      {#if reset?.id === u.id}
        <li class="reset">
          <form onsubmit={saveReset}>
            <input class="input" type="password" placeholder="Nouveau mot de passe pour {u.name}" bind:value={resetPw} autocomplete="new-password" />
            <button class="btn quiet" type="submit" disabled={resetPw.length < 8}>Enregistrer</button>
            <button class="btn danger" type="button" onclick={() => (reset = null)}>Annuler</button>
          </form>
        </li>
      {/if}
    {/each}
  </ul>
  <dl class="roles">
    {#each ROLES as r (r.id)}<div><dt>{r.label}</dt><dd>{r.hint}</dd></div>{/each}
  </dl>

  <h2>Ajouter quelqu'un</h2>
  <form class="add" onsubmit={addUser}>
    <input class="input" placeholder="Prénom" bind:value={add.name} autocomplete="off" />
    <select class="input" bind:value={add.role} aria-label="Rôle">
      {#each ROLES as r (r.id)}<option value={r.id}>{r.label}</option>{/each}
    </select>
    <input class="input" type="password" placeholder="Mot de passe (8 caractères ou plus)" bind:value={add.password} autocomplete="new-password" />
    <button class="btn" type="submit" disabled={!add.name.trim() || add.password.length < 8}>Créer le compte</button>
  </form>
{/if}

<style>
  .card {
    padding: 16px;
    border-radius: var(--radius-tile);
    background: var(--surface);
    box-shadow: var(--shadow);
  }

  .card h2 {
    margin-top: 0 !important;
  }

  h2 {
    margin-top: 28px !important;
  }

  .who {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  .avatar {
    display: inline-grid;
    place-items: center;
    flex: none;
    width: 46px;
    height: 46px;
    border-radius: 50%;
    background: var(--ink);
    color: var(--bg);
    font-weight: 600;
    font-size: 19px;
  }

  .avatar.small {
    width: 36px;
    height: 36px;
    font-size: 16px;
  }

  .txt {
    flex: 1;
    display: grid;
    min-width: 0;
  }

  .txt span {
    font-size: 14px;
    color: var(--muted);
  }

  .pw,
  .add,
  .reset form {
    display: grid;
    gap: 10px;
    max-width: 420px;
  }

  .users {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .users li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 0;
    border-bottom: 1px solid var(--line);
  }

  .users li.reset {
    display: block;
  }

  .sel {
    flex: 0 0 120px;
    min-height: 40px;
    padding: 6px 10px;
  }

  .mini {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--muted);
  }

  .roles {
    margin: 14px 0 0;
    display: grid;
    gap: 4px;
    font-size: 14px;
    color: var(--muted);
  }

  .roles div {
    display: flex;
    gap: 8px;
  }

  .roles dt {
    font-weight: 600;
    min-width: 64px;
  }

  .roles dd {
    margin: 0;
  }
</style>
