<script>
  import { tr } from '../lib/i18n.js';
  // Éditeur « Quand… Si… Alors… », en phrases. Pas une ligne de code.
  // Plusieurs « Quand » nommés (bouton 1, bouton 2…), et « une action par Quand » : chaque
  // déclencheur a son propre « Alors ». Blocs imbriqués : si/sinon, selon le Quand, répéter.
  import { untrack } from 'svelte';
  import Sheet from './Sheet.svelte';
  import Icon from './Icon.svelte';
  import {
    ALARM_PHASES,
    blankCondition,
    blankStep,
    blankTrigger,
    byTrigger,
    CONDITION_TYPES,
    DAYS,
    describeRearm,
    describeTrigger,
    EFFECTS,
    eventLabel,
    nextTriggerId,
    PRESENCE_EVENTS,
    REARM_TYPES,
    RUN_MODES,
    setByTrigger,
    STEP_TYPES,
    stateChoices,
    syncBranches,
    traceResult,
    TRIGGER_TYPES,
    VARIABLES,
  } from '../lib/automation.js';
  import { ago, slug } from '../lib/house.js';
  import { api, home, loadAutomations, toast } from '../lib/store.svelte.js';

  let { automation = null, preset = null, onclose } = $props();

  const source = untrack(() => automation ?? preset ?? { name: '', triggers: [], conditions: [], actions: [] });
  let draft = $state(structuredClone($state.snapshot(source)));
  draft.enabled ??= true;
  draft.rest_state ??= false;
  draft.mode ??= 'restart';
  draft.rearm ??= { triggers: [], conditions: [] };
  draft.rearm.triggers ??= [];
  draft.rearm.conditions ??= [];
  for (const t of draft.triggers) if (!t.id) t.id = nextTriggerId(draft.triggers);

  // Les blocs repliés ou ouverts : on garde l'écran court.
  let open = $state({
    when: true,
    if: untrack(() => draft.conditions.length > 0 || !automation),
    then: true,
    options: untrack(() => draft.mode !== 'restart'),
    state: untrack(() => !!(draft.rearm.triggers.length || draft.rearm.after_min || draft.rest_state)),
    history: false,
  });
  const automationsList = $derived(home.automations.filter((a) => a.id !== automation?.id).sort(byName));
  let saving = $state(false);

  const all = $derived(Object.values(home.entities).filter((e) => !e.hidden && !e.capabilities?.auto));
  const byName = (a, b) => a.name.localeCompare(b.name, 'fr');
  const stateEntities = $derived(
    all.filter((e) => ['light', 'switch', 'binary_sensor', 'scene', 'person', 'group', 'automation', 'sensor', 'cover', 'lock', 'climate', 'fan', 'siren'].includes(e.domain)).sort(byName),
  );
  const numericEntities = $derived(all.filter((e) => e.domain === 'sensor' && typeof e.state?.value === 'number').sort(byName));
  const controls = $derived(all.filter((e) => ['light', 'switch', 'group', 'fan', 'siren'].includes(e.domain)).sort(byName));
  const lights = $derived(all.filter((e) => e.domain === 'light').sort(byName));
  const scenes = $derived(all.filter((e) => e.domain === 'scene').sort(byName));

  const label = (e) =>
    e.domain === 'automation' ? tr('{name} (automatisme)', { name: e.name }) : e.domain === 'group' ? tr('{name} (groupe)', { name: e.name }) : e.room ? `${e.name} (${e.room})` : e.name;
  const count = (n) => (n ? String(n) : '');
  const perTrigger = $derived(byTrigger(draft));

  // ---- Boutons et télécommandes -------------------------------------------------
  let devices = $state([]);
  let lastEvent = null;
  let detecting = $state(null);
  async function loadDevices() {
    try {
      const r = await api('/automation_devices');
      devices = r.devices ?? [];
      return r;
    } catch {
      return null;
    }
  }
  $effect(() => {
    loadDevices().then((r) => (lastEvent = r?.last?.at ?? null));
  });

  /** « Détecter » : on attend la prochaine action reçue, quel que soit l'appareil. */
  async function detect(t) {
    if (detecting) return;
    detecting = t;
    const since = (await loadDevices())?.last?.at ?? lastEvent;
    const end = Date.now() + 60000;
    while (detecting === t && Date.now() < end) {
      await new Promise((r) => setTimeout(r, 1000));
      const r = await loadDevices();
      if (r?.last && r.last.at !== since) {
        t.device_id = r.last.device_id;
        t.event = r.last.event;
        lastEvent = r.last.at;
        toast(tr("Reçu : {name} · {event}", { name: r.last.name, event: eventLabel(r.last.event) }));
        detecting = null;
        return;
      }
    }
    if (detecting === t) {
      detecting = null;
      toast(tr("Rien reçu. Vérifiez que l'appareil est bien appairé."), 'alert');
    }
  }

  const deviceOf = (id) => devices.find((d) => d.device_id === id);

  /** Un « Quand » par action de la télécommande, chacun avec son « Alors ». */
  function splitByEvents(t) {
    const d = deviceOf(t.device_id);
    if (!d?.events?.length) return;
    const at = draft.triggers.indexOf(t);
    const made = d.events.map((ev, k) => ({
      id: k === 0 ? t.id : '',
      name: eventLabel(ev),
      type: 'device',
      device_id: d.device_id,
      event: ev,
    }));
    draft.triggers.splice(at, 1, ...made);
    for (const x of draft.triggers) if (!x.id) x.id = nextTriggerId(draft.triggers);
    if (!byTrigger(draft)) setByTrigger(draft, true);
    else syncBranches(draft);
  }

  function addTrigger(type) {
    draft.triggers.push({ id: nextTriggerId(draft.triggers), ...blankTrigger(type) });
    syncBranches(draft);
  }

  function removeTrigger(i) {
    draft.triggers.splice(i, 1);
    syncBranches(draft);
  }

  const triggerTitle = (t) => t.name || describeTrigger({ ...t, name: '' }, home.entities, devices);

  function toggleDay(list, d) {
    const i = list.indexOf(d);
    if (i >= 0) list.splice(i, 1);
    else list.push(d);
    list.sort();
  }

  function toggleIn(list, v) {
    const i = list.indexOf(v);
    if (i >= 0) list.splice(i, 1);
    else list.push(v);
  }

  function move(list, i) {
    if (i <= 0) return;
    const [x] = list.splice(i, 1);
    list.splice(i - 1, 0, x);
  }

  // ---- Historique ---------------------------------------------------------------
  let traces = $state(null);
  async function loadTraces() {
    if (!automation) return;
    try {
      traces = await api(`/automations/${automation.id}/traces`);
    } catch (e) {
      toast(e.message, 'alert');
    }
  }
  $effect(() => {
    if (!open.history) return;
    loadTraces();
    const t = setInterval(loadTraces, 4000);
    return () => clearInterval(t);
  });

  // ---- Enregistrer --------------------------------------------------------------

  function clean(steps) {
    for (const s of steps) {
      if (s.type === 'command') {
        for (const k of ['brightness', 'color', 'color_temp', 'transition']) if (s[k] == null || s[k] === '') delete s[k];
        if (s.action !== 'turn_on') for (const k of ['brightness', 'color', 'color_temp', 'transition']) delete s[k];
      }
      if (s.type === 'notify' && !s.title) delete s.title;
      for (const k of ['then', 'else', 'steps', 'default']) if (Array.isArray(s[k])) clean(s[k]);
      for (const b of s.branches ?? []) clean(b.steps);
    }
  }

  async function save() {
    saving = true;
    try {
      const body = $state.snapshot(draft);
      if (!body.rearm.after_min) delete body.rearm.after_min;
      for (const t of body.triggers) {
        if (!t.name) delete t.name;
        if (t.type === 'device' && !t.event) delete t.event;
      }
      clean(body.actions);
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

  async function tryNow(trigger = null) {
    try {
      await api(`/automations/${automation.id}/run`, { method: 'POST', body: trigger ? { trigger } : {} });
      toast(tr("Actions lancées"));
      open.history = true;
    } catch (e) {
      toast(e.message, 'alert');
    }
  }

  const pct = (b) => (b == null ? '' : Math.round((b / 255) * 100));
  const WHITES = [
    { v: 370, l: tr("blanc chaud") },
    { v: 250, l: tr("blanc neutre") },
    { v: 153, l: tr("blanc froid") },
  ];
</script>

{#snippet entitySelect(list, value, onpick, placeholder = tr("choisir un appareil"))}
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

{#snippet x(onclick)}
  <button class="x" {onclick} aria-label={tr("Retirer")}><Icon name="close" size={16} /></button>
{/snippet}

{#snippet triggerChips(ids)}
  <span class="chips">
    {#each draft.triggers as t, k (t.id)}
      <button type="button" class="chip" class:on={ids.includes(t.id)} aria-pressed={ids.includes(t.id)} onclick={() => toggleIn(ids, t.id)}>
        <b>{k + 1}</b> {triggerTitle(t)}
      </button>
    {/each}
  </span>
{/snippet}

{#snippet triggerBody(t)}
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
      {@render threshold(t, tr("passe au-dessus de"), tr("passe en dessous de"))}
    {:else if t.type === 'device'}
      <select class="pick" value={t.device_id} onchange={(e) => { t.device_id = e.currentTarget.value; t.event = ''; }}>
        <option value="" disabled>{tr("choisir l'appareil")}</option>
        {#each devices as d (d.device_id)}<option value={d.device_id}>{d.room ? `${d.name} (${d.room})` : d.name}</option>{/each}
        {#if t.device_id && !deviceOf(t.device_id)}<option value={t.device_id}>{t.device_id}</option>{/if}
      </select>
      {#if t.device_id}
        <span>{tr("envoie")}</span>
        <select class="pick" bind:value={t.event}>
          <option value="">{tr("n'importe quelle action")}</option>
          {#each deviceOf(t.device_id)?.events ?? [] as ev (ev)}<option value={ev}>{eventLabel(ev)}</option>{/each}
          {#if t.event && !(deviceOf(t.device_id)?.events ?? []).includes(t.event)}<option value={t.event}>{eventLabel(t.event)}</option>{/if}
        </select>
      {/if}
      <button type="button" class="btn quiet small" onclick={() => (detecting === t ? (detecting = null) : detect(t))}>
        {detecting === t ? tr("Appuyez sur le bouton… (annuler)") : tr("Détecter")}
      </button>
      {#if (deviceOf(t.device_id)?.events?.length ?? 0) > 1}
        <button type="button" class="btn quiet small" onclick={() => splitByEvents(t)}>
          {tr("Un « Quand » par action ({n})", { n: deviceOf(t.device_id).events.length })}
        </button>
      {/if}
      {#if !devices.length}
        <small>{tr("Aucun bouton connu : appuyez sur « Détecter » puis sur le bouton de l'appareil.")}</small>
      {/if}
    {:else if t.type === 'time'}
      <span>{tr("à")}</span>
      <input class="pick" type="time" bind:value={t.at} />
      <span class="days">
        {#each DAYS as d, k (d)}
          <button type="button" class="day" class:on={t.days.includes(k + 1)} aria-pressed={t.days.includes(k + 1)} onclick={() => toggleDay(t.days, k + 1)}>{d}</button>
        {/each}
      </span>
      <small>{t.days.length ? '' : tr("tous les jours")}</small>
    {:else if t.type === 'interval'}
      <span>{tr("toutes les")}</span>
      <input class="pick tiny num" type="number" min="1" max="1440" bind:value={t.minutes} />
      <span>{tr("min")}</span>
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
    {:else if t.type === 'presence'}
      <select class="pick" bind:value={t.event}>
        {#each PRESENCE_EVENTS as p (p.value)}<option value={p.value}>{p.label}</option>{/each}
      </select>
    {:else if t.type === 'alarm'}
      <span>{tr("l'alarme devient")}</span>
      <select class="pick" bind:value={t.event}>
        {#each ALARM_PHASES as p (p.value)}<option value={p.value}>{p.label}</option>{/each}
      </select>
    {:else if t.type === 'startup'}
      <span>{tr("Heim vient de démarrer (après une coupure ou une mise à jour)")}</span>
    {/if}
  </div>
{/snippet}

{#snippet threshold(o, aboveText, belowText)}
  <select class="pick" value={o.above != null ? 'above' : 'below'} onchange={(e) => {
    const v = o.above ?? o.below ?? 0;
    if (e.currentTarget.value === 'above') { o.above = v; delete o.below; } else { o.below = v; delete o.above; }
  }}>
    <option value="above">{aboveText}</option>
    <option value="below">{belowText}</option>
  </select>
  {#if o.above != null}
    <input class="pick tiny num" type="number" step="any" bind:value={o.above} />
  {:else}
    <input class="pick tiny num" type="number" step="any" bind:value={o.below} />
  {/if}
  <span>{home.entities[o.entity_id]?.capabilities?.unit ?? ''}</span>
{/snippet}

{#snippet rearmList(list)}
  {#each list as t, i (i)}
    <div class="line">
      {@render triggerBody(t)}
      {@render x(() => list.splice(i, 1))}
    </div>
    {#if i < list.length - 1}<p class="or">{tr("ou")}</p>{/if}
  {/each}
{/snippet}

{#snippet conditionList(list, depth = 0)}
  {#each list as c, i (i)}
    <div class="line">
      <div class="sentence">
        {#if c.type === 'time'}
          <span>{tr("entre")}</span>
          <input class="pick" type="time" bind:value={c.after} />
          <span>{tr("et")}</span>
          <input class="pick" type="time" bind:value={c.before} />
        {:else if c.type === 'dark'}
          <span>{tr("il fait nuit (après le coucher du soleil)")}</span>
        {:else if c.type === 'daylight'}
          <span>{tr("il fait jour")}</span>
        {:else if c.type === 'days'}
          <span>{tr("le")}</span>
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
        {:else if c.type === 'numeric'}
          {@render entitySelect(numericEntities, c.entity_id, (v) => (c.entity_id = v), tr("choisir une mesure"))}
          {@render threshold(c, tr("est au-dessus de"), tr("est en dessous de"))}
        {:else if c.type === 'presence'}
          <select class="pick" value={c.home ? '1' : '0'} onchange={(e) => (c.home = e.currentTarget.value === '1')}>
            <option value="1">{tr("quelqu'un est à la maison")}</option>
            <option value="0">{tr("personne n'est à la maison")}</option>
          </select>
        {:else if c.type === 'alarm'}
          <span>{tr("l'alarme est")}</span>
          <select class="pick" bind:value={c.is}>
            {#each ALARM_PHASES as p (p.value)}<option value={p.value}>{p.label}</option>{/each}
          </select>
        {:else if c.type === 'triggered'}
          <span>{tr("lancé par")}</span>
          {@render triggerChips(c.ids)}
        {:else if c.type === 'any' || c.type === 'not' || c.type === 'all'}
          <div class="group">
            <strong>{c.type === 'any' ? tr("Au moins une de :") : c.type === 'not' ? tr("Aucune de :") : tr("Toutes :")}</strong>
            {@render conditionList(c.conditions, depth + 1)}
            {@render adder(CONDITION_TYPES.filter((k) => depth < 3 || !['any', 'not'].includes(k.type)), (type) => c.conditions.push(blankCondition(type)), tr("+ Ajouter une condition"))}
          </div>
        {/if}
      </div>
      {@render x(() => list.splice(i, 1))}
    </div>
  {/each}
{/snippet}

{#snippet stepList(list, depth = 0)}
  {#each list as a, i (i)}
    <div class="line" class:blockstep={['if', 'choose', 'repeat', 'wait_until'].includes(a.type)}>
      <div class="sentence">
        {#if a.type === 'command'}
          <select class="pick" bind:value={a.action}>
            <option value="turn_on">{tr("allumer")}</option>
            <option value="turn_off">{tr("éteindre")}</option>
            <option value="toggle">{tr("basculer")}</option>
          </select>
          {@render entitySelect(controls, a.entity_id, (v) => (a.entity_id = v))}
          {#if a.action === 'turn_on' && home.entities[a.entity_id]?.domain === 'light'}
            <span class="opts">
              <label class="inline">
                {tr("luminosité")}
                <input class="pick tiny num" type="number" min="1" max="100" placeholder="—" value={pct(a.brightness)} onchange={(e) => (a.brightness = e.currentTarget.value === '' ? undefined : Math.round((Math.min(100, Math.max(1, Number(e.currentTarget.value))) * 255) / 100))} />
                %
              </label>
              <label class="inline">
                <input type="checkbox" checked={!!a.color} onchange={(e) => { a.color = e.currentTarget.checked ? '#ff8800' : undefined; if (a.color) a.color_temp = undefined; }} />
                {tr("couleur")}
                {#if a.color}<input type="color" bind:value={a.color} />{/if}
              </label>
              {#if !a.color}
                <select class="pick" value={a.color_temp ?? ''} onchange={(e) => (a.color_temp = e.currentTarget.value ? Number(e.currentTarget.value) : undefined)}>
                  <option value="">{tr("blanc inchangé")}</option>
                  {#each WHITES as w (w.v)}<option value={w.v}>{w.l}</option>{/each}
                </select>
              {/if}
              <label class="inline">
                {tr("fondu")}
                <input class="pick tiny num" type="number" min="0" max="3600" step="0.5" placeholder="—" value={a.transition ?? ''} onchange={(e) => (a.transition = e.currentTarget.value === '' ? undefined : Number(e.currentTarget.value))} />
                s
              </label>
            </span>
          {/if}
        {:else if a.type === 'turn_on_for'}
          <span>{tr("allumer")}</span>
          {@render entitySelect(controls, a.entity_id, (v) => (a.entity_id = v))}
          <span>{tr("pendant")}</span>
          <input class="pick tiny num" type="number" min="1" bind:value={a.minutes} />
          <span>{tr("min")}</span>
        {:else if a.type === 'effect'}
          <select class="pick" bind:value={a.effect}>
            {#each EFFECTS as f (f.id)}<option value={f.id}>{tr("effet {name}", { name: f.label })}</option>{/each}
            <option value="">{tr("arrêter l'effet")}</option>
          </select>
          <span>{tr("sur")}</span>
          {@render entitySelect(lights, a.entity_id, (v) => (a.entity_id = v), tr("choisir une lampe"))}
          {#if a.effect === 'pulse'}
            <input type="color" value={a.colors?.[0] ?? '#ff0000'} onchange={(e) => (a.colors = [e.currentTarget.value, a.colors?.[1] ?? '#0000ff'])} />
            <input type="color" value={a.colors?.[1] ?? '#0000ff'} onchange={(e) => (a.colors = [a.colors?.[0] ?? '#ff0000', e.currentTarget.value])} />
          {:else if a.effect === 'sunrise'}
            <span>{tr("en")}</span>
            <input class="pick tiny num" type="number" min="1" max="120" value={a.minutes ?? 15} onchange={(e) => (a.minutes = Number(e.currentTarget.value))} />
            <span>{tr("min")}</span>
          {/if}
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
          <span class="vars">
            {#each VARIABLES as v (v)}
              <button type="button" class="var" onclick={() => (a.message = `${a.message ?? ''}${a.message && !a.message.endsWith(' ') ? ' ' : ''}${v}`)}>{v}</button>
            {/each}
          </span>
        {:else if a.type === 'delay'}
          <span>{tr("attendre")}</span>
          <input class="pick tiny num" type="number" min="0" value={a.seconds >= 60 && a.seconds % 60 === 0 ? a.seconds / 60 : a.seconds} onchange={(e) => (a.seconds = Math.max(0, Number(e.currentTarget.value)) * (a.seconds >= 60 && a.seconds % 60 === 0 ? 60 : 1))} />
          <select class="pick" value={a.seconds >= 60 && a.seconds % 60 === 0 ? 'min' : 's'} onchange={(e) => (a.seconds = e.currentTarget.value === 'min' ? Math.max(1, a.seconds) * 60 : Math.max(1, Math.round(a.seconds / 60)))}>
            <option value="s">s</option>
            <option value="min">{tr("min")}</option>
          </select>
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
            {#each automationsList as o (o.id)}<option value={o.id}>{o.name}</option>{/each}
          </select>
        {:else if a.type === 'alarm'}
          <select class="pick" bind:value={a.op}>
            <option value="arm_away">{tr("activer l'alarme (absent)")}</option>
            <option value="arm_night">{tr("activer l'alarme (nuit)")}</option>
            <option value="disarm">{tr("désactiver l'alarme")}</option>
          </select>
          {#if a.op === 'disarm'}<small>{tr("seulement si aucun code n'est demandé")}</small>{/if}
        {:else if a.type === 'stop'}
          <span>{tr("arrêter ici")}</span>
          <input class="pick grow" bind:value={a.reason} placeholder={tr("raison (facultatif)")} />
        {:else if a.type === 'wait_until'}
          <div class="group">
            <strong>{tr("Attendre que :")}</strong>
            {@render conditionList(a.conditions, depth + 1)}
            {@render adder(CONDITION_TYPES, (type) => a.conditions.push(blankCondition(type)), tr("+ Ajouter une condition"))}
            <span class="sentence">
              <span>{tr("au plus")}</span>
              <input class="pick tiny num" type="number" min="1" value={Math.round(a.timeout_s / 60) || 1} onchange={(e) => (a.timeout_s = Math.max(1, Number(e.currentTarget.value)) * 60)} />
              <span>{tr("min, puis")}</span>
              <select class="pick" value={a.stop_on_timeout ? '1' : '0'} onchange={(e) => (a.stop_on_timeout = e.currentTarget.value === '1')}>
                <option value="0">{tr("continuer")}</option>
                <option value="1">{tr("arrêter")}</option>
              </select>
            </span>
          </div>
        {:else if a.type === 'if'}
          <div class="group">
            <strong>{tr("Si")}</strong>
            {@render conditionList(a.conditions, depth + 1)}
            {@render adder(CONDITION_TYPES, (type) => a.conditions.push(blankCondition(type)), tr("+ Ajouter une condition"))}
            <strong>{tr("Alors")}</strong>
            {@render stepList(a.then, depth + 1)}
            {@render stepAdder(a.then, depth + 1)}
            <strong>{tr("Sinon")}</strong>
            {@render stepList(a.else, depth + 1)}
            {@render stepAdder(a.else, depth + 1)}
          </div>
        {:else if a.type === 'repeat'}
          <div class="group">
            <span class="sentence">
              <strong>{tr("Répéter")}</strong>
              <input class="pick tiny num" type="number" min="1" max="1000" bind:value={a.times} />
              <span>{tr("fois")}</span>
            </span>
            {@render stepList(a.steps, depth + 1)}
            {@render stepAdder(a.steps, depth + 1)}
            <small>{tr("…tant que (facultatif) :")}</small>
            {@render conditionList(a.while, depth + 1)}
            {@render adder(CONDITION_TYPES, (type) => a.while.push(blankCondition(type)), tr("+ Ajouter une condition"))}
          </div>
        {:else if a.type === 'choose'}
          <div class="group">
            <strong>{tr("Selon le « Quand »")}</strong>
            {#each a.branches as b, k (k)}
              <div class="branch">
                <div class="sentence">
                  <strong>{tr("Cas {n}", { n: k + 1 })}</strong>
                  <span>{tr("lancé par")}</span>
                  {@render triggerChips(b.when)}
                  {@render x(() => a.branches.splice(k, 1))}
                </div>
                <small>{b.when.length ? '' : tr("(n'importe quel « Quand »)")}</small>
                {@render conditionList(b.conditions, depth + 1)}
                {@render adder(CONDITION_TYPES, (type) => b.conditions.push(blankCondition(type)), tr("+ et si…"))}
                {@render stepList(b.steps, depth + 1)}
                {@render stepAdder(b.steps, depth + 1)}
              </div>
            {/each}
            <button type="button" class="btn quiet small" onclick={() => a.branches.push({ when: [], conditions: [], steps: [] })}>{tr("+ Ajouter un cas")}</button>
            <strong>{tr("Sinon")}</strong>
            {@render stepList(a.default, depth + 1)}
            {@render stepAdder(a.default, depth + 1)}
          </div>
        {/if}
      </div>
      <span class="tools">
        {#if i > 0}<button class="x up" onclick={() => move(list, i)} aria-label={tr("Monter")}><Icon name="chevron" size={16} /></button>{/if}
        {@render x(() => list.splice(i, 1))}
      </span>
    </div>
  {/each}
{/snippet}

{#snippet stepAdder(list, depth)}
  {@render adder(STEP_TYPES.filter((s) => depth < 4 || !['if', 'choose', 'repeat'].includes(s.type)), (type) => list.push(blankStep(type)), tr("+ Ajouter une action"))}
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
    {#each draft.triggers as t, i (t.id)}
      <div class="trig">
        <div class="trig-head">
          <span class="badge">{i + 1}</span>
          <input class="pick grow name" bind:value={t.name} placeholder={tr("Nom (facultatif) : Bouton 1, Porte…")} />
          {@render x(() => removeTrigger(i))}
        </div>
        {@render triggerBody(t)}
      </div>
      {#if i < draft.triggers.length - 1}<p class="or">{tr("ou")}</p>{/if}
    {/each}
    {@render adder(TRIGGER_TYPES, addTrigger, tr("+ Ajouter un « Quand »"))}
  {/snippet}
  {@render block('when', tr("Quand"), tr("un seul suffit ; chacun est numéroté"), count(draft.triggers.length), whenBody)}

  {#snippet ifBody()}
    {@render conditionList(draft.conditions)}
    {@render adder(CONDITION_TYPES, (type) => draft.conditions.push(blankCondition(type)), tr("+ Ajouter un « Si »"))}
  {/snippet}
  {@render block('if', tr("Si"), tr("facultatif, toutes doivent être vraies"), count(draft.conditions.length), ifBody)}

  {#snippet thenBody()}
    <label class="check per">
      <input type="checkbox" checked={!!perTrigger} disabled={draft.triggers.length < 2 && !perTrigger} onchange={(e) => setByTrigger(draft, e.currentTarget.checked)} />
      <span>
        <strong>{tr("Une action différente par « Quand »")}</strong>
        <small>{tr("Alors 1 pour le Quand 1, Alors 2 pour le Quand 2… (ex. les boutons d'une télécommande)")}</small>
      </span>
    </label>
    {#if perTrigger}
      {#each perTrigger.branches as b, k (b.when[0])}
        {@const t = draft.triggers.find((x) => x.id === b.when[0])}
        {#if t}
          <div class="branch per-branch">
            <div class="branch-head"><span class="badge">{draft.triggers.indexOf(t) + 1}</span><strong>{tr("Alors {n}", { n: draft.triggers.indexOf(t) + 1 })}</strong><small>{triggerTitle(t)}</small>
              {#if automation?.triggers?.some((o) => o.id === t.id)}
                <button type="button" class="x play" onclick={() => tryNow(t.id)} title={tr("Essayer cet « Alors »")} aria-label={tr("Essayer cet « Alors »")}><Icon name="play" size={16} /></button>
              {/if}
            </div>
            {#if b.conditions.length}{@render conditionList(b.conditions, 1)}{/if}
            {@render stepList(b.steps, 1)}
            <span class="row">
              {@render stepAdder(b.steps, 1)}
              {@render adder(CONDITION_TYPES, (type) => b.conditions.push(blankCondition(type)), tr("+ et si…"))}
            </span>
          </div>
        {/if}
      {/each}
    {:else}
      {@render stepList(draft.actions)}
      {@render stepAdder(draft.actions, 0)}
    {/if}
  {/snippet}
  {@render block('then', tr("Alors"), perTrigger ? tr("un « Alors » par « Quand »") : tr("dans l’ordre"), count(perTrigger ? perTrigger.branches.length : draft.actions.length), thenBody)}

  {#snippet optionsBody()}
    <div class="field">
      <span>{tr("S'il est relancé alors qu'il tourne encore")}</span>
      <select class="pick" bind:value={draft.mode}>
        {#each RUN_MODES as m (m.value)}<option value={m.value}>{m.label}</option>{/each}
      </select>
      <small>{RUN_MODES.find((m) => m.value === draft.mode)?.hint}</small>
    </div>
  {/snippet}
  {@render block('options', tr("Relance"), RUN_MODES.find((m) => m.value === draft.mode)?.label, '', optionsBody)}

  {#snippet stateBody()}
    <p class="muted explain">
      {tr("Chaque automatisme a un état vrai / faux. Au repos il vaut")}
      <strong>{draft.rest_state ? tr("vrai") : tr("faux")}</strong> {tr("; quand ses actions se lancent, il passe à")}
      <strong>{draft.rest_state ? tr("faux") : tr("vrai")}</strong> {tr("et y reste jusqu'au réarmement. Un bouton de l'accueil peut l'afficher (couleur, texte) et le réarmer d'un appui long.")}
    </p>
    <div class="field">
      <span>{tr("État au repos")}</span>
      <div class="seg">
        <button class:on={!draft.rest_state} onclick={() => (draft.rest_state = false)}>{tr("Faux")}</button>
        <button class:on={draft.rest_state} onclick={() => (draft.rest_state = true)}>{tr("Vrai")}</button>
      </div>
    </div>
    <h4>{tr("Réarmer tout seul quand")} <small>{tr("(facultatif, un seul suffit)")}</small></h4>
    {@render rearmList(draft.rearm.triggers)}
    {@render adder(REARM_TYPES, (type) => draft.rearm.triggers.push(blankTrigger(type)), tr("+ Ajouter un réarmement"))}
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
      {@render adder(CONDITION_TYPES.filter((c) => c.type !== 'triggered'), (type) => draft.rearm.conditions.push(blankCondition(type)), tr("+ Ajouter une condition"))}
    {/if}
    {#if automation && automation.state !== automation.rest_state}
      <p class="fired">{tr("Actuellement déclenché.")}</p>
    {/if}
  {/snippet}
  {@render block(
    'state',
    tr("État et réarmement"),
    draft.rearm.triggers.length || draft.rearm.after_min ? describeRearm(draft.rearm, home.entities, devices) : tr("boîte aux lettres, lessive finie…"),
    '',
    stateBody,
  )}

  {#if automation}
    {#snippet historyBody()}
      {#if !traces}
        <p class="muted">{tr("Chargement…")}</p>
      {:else if !traces.length}
        <p class="muted">{tr("Pas encore d'exécution depuis le dernier démarrage de Heim.")}</p>
      {:else}
        {#each traces as r (r.at + r.result)}
          {@const res = traceResult(r)}
          <details class="trace {res.tone}">
            <summary>
              <span class="dot"><Icon name={res.icon} size={14} /></span>
              <span class="when">{ago(r.at, home.now)}</span>
              <strong>{r.ctx.trigger ?? '—'}</strong>
              <span class="res">{res.text}{r.detail ? ` : ${r.detail}` : ''}</span>
            </summary>
            {#if r.ctx.event || r.ctx.device}<p class="muted small">{r.ctx.device ?? ''} {r.ctx.event ? `· ${eventLabel(r.ctx.event)}` : ''}</p>{/if}
            <ol class="steps">
              {#each r.steps as s, k (k)}
                <li class:bad={!s.ok}>
                  <code>{s.path}</code>
                  <span>{s.label}</span>
                  {#if s.detail}<small>{s.detail}</small>{/if}
                </li>
              {/each}
            </ol>
          </details>
        {/each}
      {/if}
    {/snippet}
    {@render block('history', tr("Historique"), tr("les dernières exécutions, pas à pas"), '', historyBody)}
  {/if}

  <label class="check"><input type="checkbox" bind:checked={draft.enabled} /> {tr("Actif")}</label>

  <div class="row">
    <button class="btn" onclick={save} disabled={saving || !draft.name.trim()}>{tr("Enregistrer")}</button>
    {#if automation}
      <button class="btn quiet" onclick={() => tryNow()}>{tr("Essayer maintenant")}</button>
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

  .n,
  .badge {
    min-width: 22px;
    padding: 0 7px;
    border-radius: 11px;
    background: var(--ink);
    color: var(--bg);
    font-size: 12px;
    font-weight: 700;
    text-align: center;
    line-height: 22px;
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

  .trig {
    padding: 8px 8px 10px 10px;
    border-radius: 14px;
    background: var(--surface);
    margin-bottom: 8px;
  }

  .trig-head,
  .branch-head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }

  .branch-head small {
    color: var(--muted);
  }

  .name {
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

  .line .line {
    background: color-mix(in srgb, var(--surface-2) 60%, var(--surface));
  }

  .sentence {
    flex: 1;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 8px;
    min-width: 0;
  }

  .group,
  .branch {
    display: grid;
    gap: 8px;
    width: 100%;
    min-width: 0;
  }

  .group > *,
  .branch > *,
  .line {
    min-width: 0;
  }

  .line.blockstep {
    padding-left: 8px;
  }

  .branch {
    padding: 10px;
    border-radius: 14px;
    border: 1px dashed var(--line);
  }

  .per-branch {
    border-style: solid;
    background: var(--surface);
    margin-bottom: 10px;
  }

  .opts {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 12px;
    width: 100%;
    font-size: 14px;
  }

  .chips,
  .vars {
    display: inline-flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .chip,
  .var {
    min-height: 32px;
    padding: 0 10px;
    border-radius: 16px;
    border: 1px solid var(--line);
    font-size: 13px;
  }

  .chip.on {
    background: var(--ink);
    color: var(--bg);
    border-color: var(--ink);
  }

  .var {
    font-family: ui-monospace, monospace;
    font-size: 12px;
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
    min-width: 160px;
  }

  .btn.small {
    min-height: 36px;
    font-size: 14px;
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

  .tools {
    display: flex;
    flex-direction: column;
    gap: 2px;
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

  .x.play {
    margin-left: auto;
    color: var(--ink);
  }

  .x.up {
    transform: rotate(-90deg);
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

  .check.per {
    align-items: flex-start;
    margin-bottom: 12px;
  }

  .check.per span {
    display: grid;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .trace {
    margin-bottom: 6px;
    padding: 8px 10px;
    border-radius: 12px;
    background: var(--surface);
  }

  .trace summary {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 8px;
    cursor: pointer;
  }

  .dot {
    display: inline-grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    background: var(--surface-2);
    color: var(--muted);
  }

  .trace.ok .dot {
    background: var(--moss);
    color: #fff;
  }

  .trace.bad .dot {
    background: var(--alert);
    color: #fff;
  }

  .trace.wait .dot {
    background: #d9a441;
    color: #fff;
  }

  .when,
  .res {
    font-size: 13px;
    color: var(--muted);
  }

  .steps {
    margin: 8px 0 0;
    padding-left: 0;
    list-style: none;
    display: grid;
    gap: 4px;
    font-size: 14px;
  }

  .steps li {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 8px;
  }

  .steps li.bad span {
    color: var(--alert);
  }

  .steps code {
    min-width: 34px;
    color: var(--muted);
    font-size: 12px;
  }

  .steps small {
    flex-basis: 100%;
    padding-left: 42px;
  }

  .small {
    font-size: 13px;
    margin: 4px 0 0;
  }
</style>
