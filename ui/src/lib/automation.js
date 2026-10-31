// Automatismes : vocabulaire, valeurs proposées selon l'appareil, modèles prêts à l'emploi,
// et des phrases lisibles pour la liste et l'historique.

import { tr } from './i18n.js';
import { kindOf } from './format.js';

const BINARY_WORDS = {
  contact: [tr("Ouverte"), tr("Fermée")],
  occupancy: [tr("Présence"), tr("Personne")],
  presence: [tr("Présence"), tr("Personne")],
  water_leak: [tr("Fuite"), tr("Au sec")],
  smoke: [tr("Fumée"), tr("Normal")],
  vibration: [tr("Vibration"), tr("Calme")],
};

/** Les états qu'on peut attendre d'un appareil, avec leur nom en français. */
export function stateChoices(entity) {
  if (!entity) return [];
  if (entity.domain === 'automation') {
    return [
      { value: 'on', label: tr("vrai") },
      { value: 'off', label: tr("faux") },
    ];
  }
  if (entity.domain === 'light' || entity.domain === 'switch' || entity.domain === 'scene' || entity.domain === 'group') {
    return [
      { value: 'on', label: entity.domain === 'scene' ? 'active' : tr("allumé") },
      { value: 'off', label: entity.domain === 'scene' ? 'inactive' : tr("éteint") },
    ];
  }
  if (entity.domain === 'person') {
    return [
      { value: 'home', label: tr("à la maison") },
      { value: 'away', label: tr("absent") },
    ];
  }
  if (entity.domain === 'binary_sensor') {
    const [on, off] = BINARY_WORDS[kindOf(entity)] || ['actif', 'inactif'];
    return [
      { value: 'on', label: on.toLowerCase() },
      { value: 'off', label: off.toLowerCase() },
    ];
  }
  return [];
}

export function stateLabel(entity, value) {
  return stateChoices(entity).find((c) => c.value === value)?.label ?? String(value);
}

export const DAYS = ['lun', 'mar', 'mer', 'jeu', 'ven', 'sam', 'dim'];

export const TRIGGER_TYPES = [
  { type: 'device', label: tr("Un bouton ou une télécommande") },
  { type: 'state', label: tr("Un appareil change") },
  { type: 'numeric', label: tr("Une mesure franchit un seuil") },
  { type: 'time', label: tr("À une heure précise") },
  { type: 'interval', label: tr("Régulièrement (toutes les N minutes)") },
  { type: 'sun', label: tr("Au lever ou au coucher du soleil") },
  { type: 'presence', label: tr("Quelqu'un arrive ou part") },
  { type: 'alarm', label: tr("L'alarme change") },
  { type: 'startup', label: tr("Au démarrage de Heim") },
];

/** Pour le réarmement : pas de démarrage ni de présence globale utiles ? On garde tout. */
export const REARM_TYPES = TRIGGER_TYPES;

export const CONDITION_TYPES = [
  { type: 'time', label: tr("Entre deux heures") },
  { type: 'dark', label: tr("Il fait nuit") },
  { type: 'daylight', label: tr("Il fait jour") },
  { type: 'days', label: tr("Certains jours") },
  { type: 'state', label: tr("Un appareil est dans un état") },
  { type: 'numeric', label: tr("Une mesure est au-dessus ou en dessous") },
  { type: 'presence', label: tr("Quelqu'un est à la maison (ou personne)") },
  { type: 'alarm', label: tr("L'alarme est…") },
  { type: 'triggered', label: tr("Lancé par tel « Quand »") },
  { type: 'any', label: tr("Groupe : au moins une vraie") },
  { type: 'not', label: tr("Groupe : aucune vraie") },
];

export const STEP_TYPES = [
  { type: 'command', label: tr("Allumer, éteindre ou basculer") },
  { type: 'turn_on_for', label: tr("Allumer pour un temps") },
  { type: 'effect', label: tr("Effet lumineux") },
  { type: 'scene', label: tr("Activer une scène") },
  { type: 'notify', label: tr("Envoyer une notification") },
  { type: 'delay', label: tr("Attendre") },
  { type: 'wait_until', label: tr("Attendre que…") },
  { type: 'if', label: tr("Si… alors… sinon") },
  { type: 'choose', label: tr("Selon le « Quand » (branches)") },
  { type: 'repeat', label: tr("Répéter") },
  { type: 'alarm', label: tr("Activer ou désactiver l'alarme") },
  { type: 'automation', label: tr("Réarmer, activer ou lancer un automatisme") },
  { type: 'stop', label: tr("Arrêter ici") },
];

export const ALARM_PHASES = [
  { value: 'armed', label: tr("activée") },
  { value: 'disarmed', label: tr("désactivée") },
  { value: 'arming', label: tr("en cours d'activation") },
  { value: 'pending', label: tr("en délai d'entrée") },
  { value: 'triggered', label: tr("déclenchée (intrusion)") },
];

export const PRESENCE_EVENTS = [
  { value: 'first_home', label: tr("le premier rentre (maison vide)") },
  { value: 'last_away', label: tr("le dernier part (maison vide)") },
  { value: 'arrive', label: tr("quelqu'un rentre") },
  { value: 'leave', label: tr("quelqu'un part") },
];

export const EFFECTS = [
  { id: 'breathe', label: tr("Respiration") },
  { id: 'rainbow', label: tr("Arc-en-ciel") },
  { id: 'candle', label: tr("Bougie") },
  { id: 'sunrise', label: tr("Lever de soleil") },
  { id: 'pulse', label: tr("Pulsation") },
  { id: 'alert', label: tr("Alerte") },
];

export const RUN_MODES = [
  { value: 'restart', label: tr("Recommencer depuis le début"), hint: tr("l'exécution en cours s'arrête (idéal pour « mouvement → allumer, attendre, éteindre »)") },
  { value: 'single', label: tr("Ignorer"), hint: tr("tant qu'il tourne, les nouveaux déclenchements sont ignorés") },
  { value: 'queued', label: tr("Mettre en file"), hint: tr("chaque déclenchement s'exécute après le précédent") },
  { value: 'parallel', label: tr("En parallèle"), hint: tr("plusieurs exécutions en même temps") },
];

export const VARIABLES = ['{trigger}', '{entity}', '{value}', '{device}', '{event}', '{time}', '{date}'];

// ---- Actions des boutons et télécommandes ----------------------------------------

const EVENT_PHRASES = {
  single: tr("appui"),
  click: tr("appui"),
  press: tr("appui"),
  double: tr("double appui"),
  triple: tr("triple appui"),
  quadruple: tr("quadruple appui"),
  many: tr("appuis multiples"),
  hold: tr("appui long"),
  long: tr("appui long"),
  long_press: tr("appui long"),
  release: tr("relâché"),
  hold_release: tr("relâché après appui long"),
  long_release: tr("relâché après appui long"),
  on: tr("allumer"),
  off: tr("éteindre"),
  toggle: tr("basculer"),
  shake: tr("secoué"),
  tap: tr("tapoté"),
  slide: tr("glissé"),
  flip90: tr("retourné d'un quart"),
  flip180: tr("retourné"),
  fall: tr("chute"),
  wakeup: tr("réveil"),
  rotate_left: tr("tourné à gauche"),
  rotate_right: tr("tourné à droite"),
  brightness_up: tr("luminosité +"),
  brightness_down: tr("luminosité −"),
  brightness_step_up: tr("luminosité +"),
  brightness_step_down: tr("luminosité −"),
  brightness_move_up: tr("luminosité + (maintenu)"),
  brightness_move_down: tr("luminosité − (maintenu)"),
  brightness_stop: tr("luminosité : relâché"),
  color_temperature_step_up: tr("blanc plus froid"),
  color_temperature_step_down: tr("blanc plus chaud"),
  arrow_left_click: tr("flèche gauche"),
  arrow_right_click: tr("flèche droite"),
  arrow_left_hold: tr("flèche gauche, appui long"),
  arrow_right_hold: tr("flèche droite, appui long"),
  arrow_left_release: tr("flèche gauche, relâché"),
  arrow_right_release: tr("flèche droite, relâché"),
};

const SIDE = { left: tr("gauche"), right: tr("droite"), top: tr("haut"), bottom: tr("bas"), up: tr("haut"), down: tr("bas"), center: tr("centre"), middle: tr("milieu"), both: tr("les deux") };

/** « 1_single » → « bouton 1 · appui », « double_left » → « gauche · double appui ». */
export function eventLabel(ev) {
  if (!ev) return tr("n'importe quelle action");
  if (EVENT_PHRASES[ev]) return EVENT_PHRASES[ev];
  const parts = ev.split('_');
  const where = [];
  const rest = [];
  for (let i = 0; i < parts.length; i++) {
    const p = parts[i];
    if (/^\d+$/.test(p)) where.push(tr("bouton {n}", { n: p }));
    else if (p === 'button' && /^\d+$/.test(parts[i + 1] ?? '')) continue;
    else if (SIDE[p]) where.push(SIDE[p]);
    else rest.push(p);
  }
  const what = rest.join('_');
  const action = EVENT_PHRASES[what] ?? what.replaceAll('_', ' ');
  return [where.join(' '), action].filter(Boolean).join(' · ');
}

// ---- Blocs vides ------------------------------------------------------------------

/** Un identifiant libre pour un nouveau « Quand ». */
export function nextTriggerId(triggers) {
  const used = new Set(triggers.map((t) => t.id));
  let n = 1;
  while (used.has(`t${n}`)) n++;
  return `t${n}`;
}

export function blankTrigger(type) {
  switch (type) {
    case 'state':
      return { type, entity_id: '', to: 'on' };
    case 'numeric':
      return { type, entity_id: '', above: 25 };
    case 'device':
      return { type, device_id: '', event: '' };
    case 'time':
      return { type, at: '07:00', days: [] };
    case 'interval':
      return { type, minutes: 15 };
    case 'presence':
      return { type, event: 'first_home' };
    case 'alarm':
      return { type, event: 'triggered' };
    case 'startup':
      return { type };
    default:
      return { type: 'sun', event: 'sunset', offset_min: 0 };
  }
}

export function blankCondition(type) {
  switch (type) {
    case 'time':
      return { type, after: '22:00', before: '06:00' };
    case 'days':
      return { type, days: [1, 2, 3, 4, 5] };
    case 'state':
      return { type, entity_id: '', is: 'on' };
    case 'numeric':
      return { type, entity_id: '', above: 20 };
    case 'presence':
      return { type, home: true };
    case 'alarm':
      return { type, is: 'armed' };
    case 'triggered':
      return { type, ids: [] };
    case 'any':
    case 'not':
    case 'all':
      return { type, conditions: [] };
    default:
      return { type };
  }
}

export function blankStep(type) {
  switch (type) {
    case 'command':
      return { type, entity_id: '', action: 'turn_on' };
    case 'turn_on_for':
      return { type, entity_id: '', minutes: 5 };
    case 'effect':
      return { type, entity_id: '', effect: 'breathe', colors: [] };
    case 'scene':
      return { type, scene_id: '', on: true };
    case 'notify':
      return { type, message: '' };
    case 'automation':
      return { type, automation_id: '', op: 'rearm' };
    case 'wait_until':
      return { type, conditions: [], timeout_s: 600, stop_on_timeout: false };
    case 'if':
      return { type, conditions: [], then: [], else: [] };
    case 'choose':
      return { type, branches: [{ when: [], conditions: [], steps: [] }], default: [] };
    case 'repeat':
      return { type, times: 3, while: [], steps: [] };
    case 'alarm':
      return { type, op: 'arm_away' };
    case 'stop':
      return { type, reason: '' };
    default:
      return { type: 'delay', seconds: 60 };
  }
}

// ---- « Une action par Quand » ---------------------------------------------------------

/** Le bloc « selon le déclencheur » créé par l'interrupteur, s'il est là. */
export function byTrigger(draft) {
  const a = draft.actions;
  return a.length === 1 && a[0].type === 'choose' && a[0].by_trigger ? a[0] : null;
}

/** Une branche par « Quand », dans le même ordre ; garde les actions déjà saisies. */
export function syncBranches(draft) {
  const c = byTrigger(draft);
  if (!c) return;
  const old = new Map(c.branches.map((b) => [b.when[0], b]));
  c.branches = draft.triggers.map((t) => old.get(t.id) ?? { when: [t.id], conditions: [], steps: [] });
}

export function setByTrigger(draft, on) {
  if (on) {
    const before = draft.actions;
    draft.actions = [{ type: 'choose', by_trigger: true, branches: [], default: [] }];
    syncBranches(draft);
    if (before.length && draft.actions[0].branches[0]) draft.actions[0].branches[0].steps = before;
  } else {
    const c = byTrigger(draft);
    if (!c) return;
    draft.actions = [...c.branches.flatMap((b) => b.steps), ...c.default];
  }
}

// ---- Modèles ----------------------------------------------------------------------

/** Modèles pour démarrer sans réfléchir ; l'utilisateur n'a qu'à choisir ses appareils. */
export const PRESETS = [
  {
    title: tr("Télécommande : un bouton, une action"),
    detail: tr("Chaque bouton fait sa propre chose : bouton 1 → salon, bouton 2 → cuisine… Choisissez la télécommande, Heim crée un « Quand » par bouton."),
    value: {
      name: tr("Télécommande"),
      mode: 'queued',
      triggers: [{ id: 't1', type: 'device', device_id: '', event: '' }],
      conditions: [],
      actions: [{ type: 'choose', by_trigger: true, branches: [{ when: ['t1'], conditions: [], steps: [] }], default: [] }],
    },
  },
  {
    title: tr("Interrupteur sans fil"),
    detail: tr("Appui : basculer la lampe. Double appui : tout éteindre. Appui long : lumière douce."),
    value: {
      name: tr("Interrupteur sans fil"),
      triggers: [
        { id: 't1', name: tr("Appui"), type: 'device', device_id: '', event: 'single' },
        { id: 't2', name: tr("Double appui"), type: 'device', device_id: '', event: 'double' },
        { id: 't3', name: tr("Appui long"), type: 'device', device_id: '', event: 'hold' },
      ],
      conditions: [],
      actions: [
        {
          type: 'choose',
          by_trigger: true,
          branches: [
            { when: ['t1'], conditions: [], steps: [{ type: 'command', entity_id: '', action: 'toggle' }] },
            { when: ['t2'], conditions: [], steps: [{ type: 'command', entity_id: '', action: 'turn_off' }] },
            { when: ['t3'], conditions: [], steps: [{ type: 'command', entity_id: '', action: 'turn_on', brightness: 50 }] },
          ],
          default: [],
        },
      ],
    },
  },
  {
    title: tr("Éclairer l'entrée la nuit"),
    detail: tr("Quand la porte s'ouvre et qu'il fait nuit, allumer pour 5 minutes."),
    value: {
      name: tr("Entrée éclairée la nuit"),
      triggers: [{ id: 't1', type: 'state', entity_id: '', to: 'on' }],
      conditions: [{ type: 'dark' }],
      actions: [{ type: 'turn_on_for', entity_id: '', minutes: 5 }],
    },
  },
  {
    title: tr("Lumière au passage"),
    detail: tr("Mouvement : allumer ; quand plus personne ne bouge depuis 2 minutes : éteindre."),
    value: {
      name: tr("Lumière au passage"),
      mode: 'restart',
      triggers: [{ id: 't1', type: 'state', entity_id: '', to: 'on' }],
      conditions: [],
      actions: [
        { type: 'command', entity_id: '', action: 'turn_on' },
        { type: 'wait_until', conditions: [{ type: 'state', entity_id: '', is: 'off' }], timeout_s: 3600, stop_on_timeout: false },
        { type: 'delay', seconds: 120 },
        { type: 'command', entity_id: '', action: 'turn_off' },
      ],
    },
  },
  {
    title: tr("Porte restée ouverte"),
    detail: tr("Prévenir si une porte reste ouverte plus de 10 minutes."),
    value: {
      name: tr("Porte restée ouverte"),
      triggers: [{ id: 't1', type: 'state', entity_id: '', to: 'on', for_s: 600 }],
      conditions: [],
      actions: [{ type: 'notify', message: tr("{entity} est ouverte depuis 10 minutes.") }],
    },
  },
  {
    title: tr("Bienvenue à la maison"),
    detail: tr("Quand le premier rentre et qu'il fait nuit, allumer l'entrée 5 minutes (présence par le Wi-Fi)."),
    value: {
      name: tr("Bienvenue à la maison"),
      triggers: [{ id: 't1', type: 'presence', event: 'first_home' }],
      conditions: [{ type: 'dark' }],
      actions: [{ type: 'turn_on_for', entity_id: '', minutes: 5 }],
    },
  },
  {
    title: tr("Tout le monde est parti"),
    detail: tr("Quand le dernier part : éteindre les lumières et activer l'alarme."),
    value: {
      name: tr("Maison vide"),
      triggers: [{ id: 't1', type: 'presence', event: 'last_away' }],
      conditions: [],
      actions: [
        { type: 'command', entity_id: '', action: 'turn_off' },
        { type: 'alarm', op: 'arm_away' },
      ],
    },
  },
  {
    title: tr("Intrusion : tout allumer"),
    detail: tr("Quand l'alarme se déclenche : allumer les lumières en effet « Alerte » et prévenir."),
    value: {
      name: tr("Intrusion"),
      triggers: [{ id: 't1', type: 'alarm', event: 'triggered' }],
      conditions: [],
      actions: [
        { type: 'effect', entity_id: '', effect: 'alert', colors: [] },
        { type: 'notify', message: tr("Alarme déclenchée à {time}") },
      ],
    },
  },
  {
    title: tr("Courrier arrivé"),
    detail: tr("Quand la boîte aux lettres s'ouvre : prévenir et lever l'état « Courrier ». Réarmé quand on relève le courrier (trappe arrière) ou à la main."),
    value: {
      name: tr("Courrier"),
      triggers: [{ id: 't1', type: 'state', entity_id: '', to: 'on' }],
      conditions: [],
      actions: [{ type: 'notify', message: tr("Le facteur est passé.") }],
      rest_state: false,
      rearm: { triggers: [{ type: 'state', entity_id: '', to: 'on' }], conditions: [] },
    },
  },
  {
    title: tr("Tout éteindre le soir"),
    detail: tr("À 23 h 30, éteindre les lumières choisies."),
    value: {
      name: tr("Extinction du soir"),
      triggers: [{ id: 't1', type: 'time', at: '23:30', days: [] }],
      conditions: [],
      actions: [{ type: 'command', entity_id: '', action: 'turn_off' }],
    },
  },
];

// ---- Phrases ----------------------------------------------------------------------

/** « Réarmé quand … », pour la liste. */
export function describeRearm(rearm, entities, devices = []) {
  const parts = (rearm?.triggers ?? []).map((t) => describeTrigger(t, entities, devices));
  if (rearm?.after_min) parts.push(tr("au bout de {v}", { v: rearm.after_min >= 60 && rearm.after_min % 60 === 0 ? `${rearm.after_min / 60} h` : `${rearm.after_min} min` }));
  return parts.join(' ou ');
}

/** « Quand Porte d'entrée devient ouverte » — pour la liste. Un « Quand » nommé garde son nom. */
export function describeTrigger(t, entities, devices = []) {
  if (t.name) return t.name;
  const name = (id) => entities[id]?.name ?? (id || '…');
  switch (t.type) {
    case 'state': {
      const base = tr("{name} devient {state}", { name: name(t.entity_id), state: stateLabel(entities[t.entity_id], t.to) });
      return t.for_s ? tr("{base} depuis {round} min", { base, round: Math.round(t.for_s / 60) }) : base;
    }
    case 'numeric': {
      const parts = [];
      if (t.above != null) parts.push(tr("dépasse {above}", { above: t.above }));
      if (t.below != null) parts.push(tr("passe sous {below}", { below: t.below }));
      return `${name(t.entity_id)} ${parts.join(' et ')}`;
    }
    case 'device': {
      const d = devices.find((x) => x.device_id === t.device_id);
      return `${d?.name ?? tr("appareil")} : ${eventLabel(t.event)}`;
    }
    case 'time': {
      const days = t.days?.length ? ` (${t.days.map((d) => DAYS[d - 1]).join(', ')})` : '';
      return tr("à {at}", { at: t.at }) + days;
    }
    case 'interval':
      return tr("toutes les {n} min", { n: t.minutes });
    case 'sun': {
      const what = t.event === 'sunrise' ? tr("au lever du soleil") : tr("au coucher du soleil");
      if (!t.offset_min) return what;
      return `${Math.abs(t.offset_min)} min ${t.offset_min < 0 ? tr("avant") : tr("après")} ${what.slice(3)}`;
    }
    case 'presence':
      return PRESENCE_EVENTS.find((p) => p.value === t.event)?.label ?? t.event;
    case 'alarm':
      return tr("l'alarme est {state}", { state: ALARM_PHASES.find((p) => p.value === t.event)?.label ?? t.event });
    case 'startup':
      return tr("au démarrage de Heim");
  }
  return '';
}

/** Résultat d'une exécution, pour l'historique. */
export function traceResult(tr0) {
  switch (tr0.result) {
    case 'ok':
      return { icon: 'check', tone: 'ok', text: tr("Terminé") };
    case 'error':
      return { icon: 'close', tone: 'bad', text: tr("Erreur") };
    case 'stopped':
      return { icon: 'close', tone: 'muted', text: tr("Arrêté") };
    case 'aborted':
      return { icon: 'close', tone: 'muted', text: tr("Interrompu") };
    case 'skipped':
      return { icon: 'close', tone: 'muted', text: tr("Pas lancé") };
    default:
      return { icon: 'timer', tone: 'wait', text: tr("En cours") };
  }
}
