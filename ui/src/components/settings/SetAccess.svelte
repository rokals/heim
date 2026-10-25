<script>
  import { setToken, toast } from '../../lib/store.svelte.js';

  let token = $state('');
</script>

<p class="muted">Si Heim a été lancé avec un code d'accès (HEIM_TOKEN), entrez-le ici une fois : ce navigateur s'en souviendra.</p>
<p class="muted">Plusieurs utilisateurs avec chacun leurs droits : prévu pour une prochaine version.</p>

<form
  onsubmit={(e) => {
    e.preventDefault();
    setToken(token);
    toast('Code enregistré');
    token = '';
  }}
>
  <label class="field">
    <span>Code d'accès</span>
    <input class="input" type="password" bind:value={token} autocomplete="current-password" />
  </label>
  <button class="btn" type="submit">Enregistrer</button>
</form>
