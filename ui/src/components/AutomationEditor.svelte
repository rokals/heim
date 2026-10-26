<script>
  import { tr } from '../lib/i18n.js';
  // Éditeur « Quand… Si… Alors… », en phrases. Pas une ligne de code.
  import { untrack } from 'svelte';
  import Sheet from './Sheet.svelte';
  import Icon from './Icon.svelte';
  import {
    blankCondition,
    blankStep,
    blankTrigger,
    CONDITION_TYPES,
    DAYS,
    describeRearm,
    STEP_TYPES,
    stateChoices,
    TRIGGER_TYPES,
  } from '../lib/automation.js';
  import { slug } from '../lib/house.js';
  import { api, home, loadAutomations, toast } from '../lib/store.svelte.js';

  let { automation = null, preset = null, onclose } = $props();

  const source = untrack(() => automation ?? preset ?? { name: '', triggers: [], conditions: [], actions: [] });
  let draft = $state(structuredClone($state.snapshot(source)));
  draft.enabled ??= true;
  draft.rest_state ??= false;
  draft.rearm ??= { triggers: [], conditions: [] };
  draft.rearm.triggers ??= [];
  draft.rearm.conditions ??= [];
  // Les blocs repliés ou ouverts : on garde l'écran court, comme on aime travailler.
  let open = $state({
    when: true,
    if: untrack(() => draft.conditions.length > 0 || !automation),
    then: true,
    state: untrack(() => !!(draft.rearm.triggers.length || draft.rearm.after_min || draft.rest_state)),
  });
  const automationsList = $derived(home.automations.filter((a) => a.id !== automation?.id).sort(byName));
  let saving = $state(false);

  const all = $derived(Object.values(home.entities).filter((e) => !e.hidden && !e.capabilities?.auto));
  const byName = (a, b) => a.name.localeCompare(b.name, 'fr');
  const stateEntities = $derived(
    all.filter((e) => ['light', 'switch', 'binary_sensor', 'scene', 'person', 'group', 'automation'].includes(e.domain)).sort(byName),
  );
  const numericEntities = $derived(all.filter((e) => e.domain === 'sensor' && typeof e.state?.value === 'number').sort(byName));
  const controls = $derived(all.filter((e) => ['light', 'switch', 'group'].includes(e.domain)).sort(byName));
  const scenes = $derived(all.filter((e) => e.domain === 'scene').sort(byName));

  const label = (e) =>
    e.domain === 'automation' ? tr('{name} (automatisme)', { name: e.name }) : e.domain === 'group' ? tr('{name} (groupe)', { name: e.name }) : e.room ? `${e.name} (${e.room})` : e.name;
  const count = (n) => (n ? String(n) : '');

  function toggleDay(list, d) {
    const i = list.indexOf(d);
    if (i >= 0) list.splice(i, 1);
    else list.push(d);
    list.sort();
  }

  async function save() {
    saving = true;
    try {
      const body = $state.snapshot(draft);
      if (!body.rearm.after_min) delete body.rearm.after_min;
      const id = automation?.id ?? slug(body.name);
      await api(`/automations/${id || 'automatisme'}`, { method: 'PUT', body });
      await loadAutomations();
      toast(tr("Automatisme enregistré"));
      onclose();
    } catch (e) {
      toast(e.message, 'alert');
    } finally {
      saving = false;
    }
  }

  async function remove() {
    if (!automation || !confirm(tr("Supprimer « {name} » ?", { name: automation.name }))) return;
    await api(`/automations/${automation.id}`, { method: 'DELETE' }).catch((e) => toast(e.message, 'alert'));
    await loadAutomations();
    onclose();
  }

  async function tryNow() {
    try {
      await api(`/automations/${automation.id}/run`, { method: 'POST' });
      toast(tr("Actions lancées"));
    } catch (e) {
      toast(e.message, 'alert');
    }
  }
</script>

{#snippet entitySelect(list, value, onpick, placeholder = 'choisir un appareil')}
  <select class="pick" value={value} onchange={(e) => onpick(e.currentTarget.value)}>
    <option value="" disabled>{placeholder}</option>
    {#each list as e (e.entity_id)}<option value={e.entity_id}>{label(e)}</option>{/each}
  </select>
{/snippet}

{#snippet adder(types, onadd, text)}
  <select class="adder" value="" onchange={(e) => { onadd(e.currentTarget.value); e.currentTarget.value = ''; }} aria-label={text}>
    <option value="" disabled>{text}</option>
    {#each types as t (t.type)}<option value={t.type}>{t.label}</option>{/each}
  </select>
{/snippet}

{#snippet triggerList(list, word)}
  {#each list as t, i (i)}
    <div class="line">
      <div class="sentence">
        {#if t.type === 'state'}
          {@render entitySelect(stateEntities, t.entity_id, (v) => (t.entity_id = v))}
          <span>{tr("devient")}</span>
          {@const choices = stateChoices(home.entities[t.entity_id])}
          {#if choices.length}
            <select class="pick" bind:value={t.to}>
              {#each choices as c (c.value)}<option value={c.value}>{c.label}</option>{/each}
            </select>
          {:else}
            <input class="pick small" bind:value={t.to} placeholder={tr("valeur")} />
          {/if}
          <label class="inline">
            <input type="checkbox" checked={!!t.for_s} onchange={(e) => (t.for_s = e.currentTarget.checked ? 600 : undefined)} />
            {tr("depuis")}
          </label>
          {#if t.for_s}
            <input class="pick tiny num" type="number" min="1" value={Math.round(t.for_s / 60)} onchange={(e) => (t.for_s = Math.max(1, Number(e.currentTarget.value)) * 60)} />
            <span>{tr("min")}</span>
          {/if}
        {:else if t.type === 'numeric'}
          {@render entitySelect(numericEntities, t.entity_id, (v) => (t.entity_id = v), tr("choisir une mesure"))}
          <select class="pick" value={t.above != null ? 'above' : 'below'} onchange={(e) => {
            const v = t.above ?? t.below ?? 0;
            if (e.currentTarget.value === 'above') { t.above = v; delete t.below; } else { t.below = v; delete t.above; }
          }}>
            <option value="above">{tr("passe au-dessus de")}</option>
            <option value="below">{tr("passe en dessous de")}</option>
          </select>
          {#if t.above != null}
            <input class="pick tiny num" type="number" step="any" bind:value={t.above} />
          {:else}
            <input class="pick tiny num" type="number" step="any" bind:value={t.below} />
          {/if}
          <span>{home.entities[t.entity_id]?.capabilities?.unit ?? ''}</span>
        {:else if t.type === 'time'}
          <span>à</span>
          <input class="pick" type="time" bind:value={t.at} />
          <span class="days">
            {#each DAYS as d, k (d)}
              <button type="button" class="day" class:on={t.days.includes(k + 1)} aria-pressed={t.days.includes(k + 1)} onclick={() => toggleDay(t.days, k + 1)}>{d}</button>
            {/each}
          </span>
          <small>{t.days.length ? '' : tr("tous les jours")}</small>
        {:else if t.type === 'sun'}
          <input class="pick tiny num" type="number" min="0" max="360" value={Math.abs(t.offset_min)} onchange={(e) => (t.offset_min = Math.sign(t.offset_min || 1) * Number(e.currentTarget.value))} />
          <span>{tr("min")}</span>
          <select class="pick" value={t.offset_min < 0 ? 'before' : 'after'} onchange={(e) => (t.offset_min = (e.currentTarget.value === 'before' ? -1 : 1) * Math.abs(t.offset_min))}>
            <option value="before">{tr("avant")}</option>
            <option value="after">{tr("après")}</option>
          </select>
          <select class="pick" bind:value={t.event}>
            <option value="sunset">{tr("le coucher du soleil")}</option>
            <option value="sunrise">{tr("le lever du soleil")}</option>
          </select>
        {/if}
      </div>
      <button class="x" onclick={() => list.splice(i, 1)} aria-label={tr("Retirer")}><Icon name="close" size={16} /></button>
    </div>
    {#if i < list.length - 1}<p class="or">{word}</p>{/if}
  {/each}
{/snippet}

{#snippet conditionList(list)}
  {#each list as c, i (i)}
    <div class="line">
      <div class="sentence">
        {#if c.type === 'time'}
          <span>{tr("entre")}</span>
          <input class="pick" type="time" bind:value={c.after} />
          <span>et</span>
          <input class="pick" type="time" bind:value={c.before} />
        {:else if c.type === 'dark'}
          <span>{tr("il fait nuit (après le coucher du soleil)")}</span>
        {:else if c.type === 'daylight'}
          <span>{tr("il fait jour")}</span>
        {:else if c.type === 'days'}
          <span>le</span>
          <span class="days">
            {#each DAYS as d, k (d)}
              <button type="button" class="day" class:on={c.days.includes(k + 1)} aria-pressed={c.days.includes(k + 1)} onclick={() => toggleDay(c.days, k + 1)}>{d}</button>
            {/each}
          </span>
        {:else if c.type === 'state'}
          {@render entitySelect(stateEntities, c.entity_id, (v) => (c.entity_id = v))}
          <span>{tr("est")}</span>
          {@const choices = stateChoices(home.entities[c.entity_id])}
          {#if choices.length}
            <select class="pick" bind:value={c.is}>
              {#each choices as ch (ch.value)}<option value={ch.value}>{ch.label}</option>{/each}
            </select>
          {:else}
            <input class="pick small" bind:value={c.is} />
          {/if}
        {/if}
      </div>
      <button class="x" onclick={() => list.splice(i, 1)} aria-label={tr("Retirer")}><Icon name="close" size={16} /></button>
    </div>
  {/each}
{/snippet}

{#snippet block(key, title, hint, n, body)}
  <section class="block" class:closed={!open[key]}>
    <button class="block-head" onclick={() => (open[key] = !open[key])} aria-expanded={open[key]}>
      <span class="chev" class:down={open[key]} aria-hidden="true"><Icon name="chevron" size={18} /></span>
      <strong>{title}</strong>
      {#if n}<span class="n">{n}</span>{/if}
      {#if hint}<small>{hint}</small>{/if}
    </button>
    {#if open[key]}
      <div class="block-body">{@render body()}</div>
    {/if}
  </section>
{/snippet}

<Sheet title={automation ? automation.name : tr("Nouvel automatisme")} {onclose}>
  <label class="field">
    <span>{tr("Nom")}</span>
    <input class="input" bind:value={draft.name} placeholder={tr("Entrée éclairée la nuit")} />
  </label>

  {#snippet whenBody()}
    {@render triggerList(draft.triggers, 'ou')}
    {@render adder(TRIGGER_TYPES, (type) => draft.triggers.push(blankTrigger(type)), tr("+ Ajouter un « Quand »"))}
  {/snippet}
  {@render block('when', tr("Quand"), tr("un seul suffit"), count(draft.triggers.length), whenBody)}

  {#snippet ifBody()}
    {@render conditionList(draft.conditions)}
    {@render adder(CONDITION_TYPES, (type) => draft.conditions.push(blankCondition(type)), tr("+ Ajouter un « Si »"))}
  {/snippet}
  {@render block('if', tr("Si"), tr("facultatif, toutes doivent être vraies"), count(draft.conditions.length), ifBody)}

  {#snippet thenBody()}
    {#each draft.actions as a, i (i)}
      <div class="line">
        <div class="sentence">
          {#if a.type === 'command'}
            <select class="pick" bind:value={a.action}>
              <option value="turn_on">{tr("allumer")}</option>
              <option value="turn_off">{tr("éteindre")}</option>
              <option value="toggle">{tr("basculer")}</option>
            </select>
            {@render entitySelect(controls, a.entity_id, (v) => (a.entity_id = v))}
          {:else if a.type === 'turn_on_for'}
            <span>{tr("allumer")}</span>
            {@render entitySelect(controls, a.entity_id, (v) => (a.entity_id = v))}
            <span>{tr("pendant")}</span>
            <input class="pick tiny num" type="number" min="1" bind:value={a.minutes} />
            <span>{tr("min")}</span>
          {:else if a.type === 'scene'}
            <select class="pick" bind:value={a.on}>
              <option value={true}>{tr("activer")}</option>
              <option value={false}>{tr("désactiver")}</option>
            </select>
            <select class="pick" bind:value={a.scene_id}>
              <option value="" disabled>{tr("choisir une scène")}</option>
              {#each scenes as s (s.entity_id)}<option value={s.entity_id.slice(6)}>{s.name}</option>{/each}
            </select>
          {:else if a.type === 'notify'}
            <span>{tr("prévenir :")}</span>
            <input class="pick grow" bind:value={a.message} placeholder={tr("La porte est ouverte")} />
          {:else if a.type === 'delay'}
            <span>{tr("attendre")}</span>
            <input class="pick tiny num" type="number" min="1" value={Math.round(a.seconds / 60)} onchange={(e) => (a.seconds = Math.max(1, Number(e.currentTarget.value)) * 60)} />
            <span>{tr("min")}</span>
          {:else if a.type === 'automation'}
            <select class="pick" bind:value={a.op}>
              <option value="rearm">{tr("réarmer")}</option>
              <option value="enable">{tr("activer")}</option>
              <option value="disable">{tr("désactiver")}</option>
              <option value="run">{tr("lancer")}</option>
            </select>
            <select class="pick" bind:value={a.automation_id}>
              <option value="" disabled>{tr("choisir un automatisme")}</option>
              {#if automation}<option value={automation.id}>{tr("celui-ci")}</option>{/if}
              {#each automationsList as x (x.id)}<option value={x.id}>{x.name}</option>{/each}
            </select>
          {/if}
        </div>
        <button class="x" onclick={() => draft.actions.splice(i, 1)} aria-label={tr("Retirer")}><Icon name="close" size={16} /></button>
      </div>
    {/each}
    {@render adder(STEP_TYPES, (type) => draft.actions.push(blankStep(type)), tr("+ Ajouter un « Alors »"))}
  {/snippet}
  {@render block('then', tr("Alors"), tr("dans l’ordre"), count(draft.actions.length), thenBody)}

  {#snippet stateBody()}
    <p class="muted explain">
      {tr("Chaque automatisme a un état vrai / faux. Au repos il vaut")}
      <strong>{draft.rest_state ? 'vrai' : 'faux'}</strong> {tr("; quand ses actions se lancent, il passe à")}
      <strong>{draft.rest_state ? 'faux' : 'vrai'}</strong> {tr("et y reste jusqu'au réarmement. Un bouton de l'accueil peut l'afficher (couleur, texte) et le réarmer d'un appui long.")}
    </p>
    <div class="field">
      <span>{tr("État au repos")}</span>
      <div class="seg">
        <button class:on={!draft.rest_state} onclick={() => (draft.rest_state = false)}>{tr("Faux")}</button>
        <button class:on={draft.rest_state} onclick={() => (draft.rest_state = true)}>{tr("Vrai")}</button>
      </div>
    </div>
    <h4>{tr("Réarmer tout seul quand")} <small>{tr("(facultatif, un seul suffit)")}</small></h4>
    {@render triggerList(draft.rearm.triggers, 'ou')}
    {@render adder(TRIGGER_TYPES, (type) => draft.rearm.triggers.push(blankTrigger(type)), tr("+ Ajouter un réarmement"))}
    <label class="inline after">
      <input
        type="checkbox"
        checked={!!draft.rearm.after_min}
        onchange={(e) => (draft.rearm.after_min = e.currentTarget.checked ? 720 : undefined)}
      />
      {tr("ou au bout de")}
      {#if draft.rearm.after_min}
        <input class="pick tiny num" type="number" min="1" value={draft.rearm.after_min >= 60 ? draft.rearm.after_min / 60 : draft.rearm.after_min} onchange={(e) => (draft.rearm.after_min = Math.max(1, Number(e.currentTarget.value)) * (draft.rearm.after_min >= 60 ? 60 : 1))} />
        <select class="pick" value={draft.rearm.after_min >= 60 ? 'h' : 'min'} onchange={(e) => (draft.rearm.after_min = e.currentTarget.value === 'h' ? Math.max(1, Math.round(draft.rearm.after_min / 60)) * 60 : Math.min(59, draft.rearm.after_min))}>
          <option value="min">{tr("min")}</option>
          <option value="h">h</option>
        </select>
      {:else}
        <span>{tr("un certain temps")}</span>
      {/if}
    </label>
    {#if draft.rearm.triggers.length}
      <h4>{tr("À condition que")} <small>{tr("(facultatif)")}</small></h4>
      {@render conditionList(draft.rearm.conditions)}
      {@render adder(CONDITION_TYPES, (type) => draft.rearm.conditions.push(blankCondition(type)), tr("+ Ajouter une condition"))}
    {/if}
    {#if automation && automation.state !== automation.rest_state}
      <p class="fired">{tr("Actuellement déclenché.")}</p>
    {/if}
  {/snippet}
  {@render block(
    'state',
    tr("État et réarmement"),
    draft.rearm.triggers.length || draft.rearm.after_min ? describeRearm(draft.rearm, home.entities) : tr("boîte aux lettres, lessive finie…"),
    '',
    stateBody,
  )}

  <label class="check"><input type="checkbox" bind:checked={draft.enabled} /> {tr("Actif")}</label>

  <div class="row">
    <button class="btn" onclick={save} disabled={saving || !draft.name.trim()}>{tr("Enregistrer")}</button>
    {#if automation}
      <button class="btn quiet" onclick={tryNow}>{tr("Essayer maintenant")}</button>
      <button class="btn danger" onclick={remove}>{tr("Supprimer")}</button>
    {/if}
  </div>
</Sheet>

<style>
  .block {
    margin: 14px 0;
    border-radius: 18px;
    background: color-mix(in srgb, var(--surface-2) 55%, transparent);
    border: 1px solid var(--line);
  }

  .block-head {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 4px 8px;
    width: 100%;
    padding: 12px 14px;
    text-align: left;
  }

  .block-head strong {
    font-size: 19px;
  }

  .block-head small {
    flex-basis: 100%;
    padding-left: 26px;
    font-size: 13px;
    color: var(--muted);
  }

  .chev {
    display: inline-grid;
    transition: transform 0.2s ease;
    color: var(--muted);
  }

  .chev.down {
    transform: rotate(90deg);
  }

  .n {
    min-width: 22px;
    padding: 0 7px;
    border-radius: 11px;
    background: var(--ink);
    color: var(--bg);
    font-size: 12px;
    font-weight: 700;
    text-align: center;
  }

  .block-body {
    padding: 0 10px 12px;
  }

  h4 {
    margin: 16px 0 6px;
    font-size: 16px;
  }

  h4 small {
    font-weight: 400;
    color: var(--muted);
  }

  .explain {
    font-size: 14px;
    margin: 0 0 8px;
  }

  .seg {
    display: flex;
    gap: 6px;
  }

  .seg button {
    min-height: 40px;
    padding: 0 16px;
    border-radius: var(--radius-chip);
    border: 1.5px solid var(--line);
    font-weight: 600;
  }

  .seg button.on {
    background: var(--ink);
    border-color: var(--ink);
    color: var(--bg);
  }

  .after {
    margin-top: 10px;
    flex-wrap: wrap;
  }

  .fired {
    color: var(--alert);
    font-weight: 600;
  }

  .line {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 10px 8px 10px 12px;
    border-radius: 14px;
    background: var(--surface);
    margin-bottom: 8px;
  }

  .sentence {
    flex: 1;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 8px;
    min-width: 0;
  }

  .pick {
    font: inherit;
    min-height: 40px;
    max-width: 100%;
    padding: 6px 10px;
    border-radius: 10px;
    border: 1px solid var(--line);
    background: var(--bg);
    color: var(--ink);
  }

  .pick.small {
    width: 110px;
  }

  .pick.tiny {
    width: 74px;
  }

  .pick.grow {
    flex: 1;
    min-width: 180px;
  }

  .inline {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .days {
    display: inline-flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .day {
    min-width: 40px;
    min-height: 34px;
    border-radius: 10px;
    border: 1px solid var(--line);
    font-size: 13px;
  }

  .day.on {
    background: var(--ink);
    color: var(--bg);
    border-color: var(--ink);
  }

  small {
    color: var(--muted);
  }

  .x {
    flex: none;
    width: 36px;
    height: 36px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    color: var(--muted);
  }

  .or {
    margin: -2px 0 6px 12px;
    font-size: 14px;
    color: var(--muted);
  }

  .adder {
    max-width: 100%;
    font: inherit;
    font-weight: 600;
    min-height: 44px;
    padding: 0 14px;
    border-radius: var(--radius-chip);
    border: 1.5px dashed var(--line);
    background: transparent;
    color: var(--ink);
  }

  .check {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 18px;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
</style>
