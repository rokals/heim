// Met en mots les lignes du journal : « Lampe salon allumée par Alex ».

import { tr, locale } from './i18n.js';
const KIND_LABELS = {
  state: 'Appareils',
  automation: 'Automatismes',
  security: tr("Alarme"),
  integration: tr("Connexions"),
  system: tr("Système"),
};

export const KINDS = Object.entries(KIND_LABELS).map(([id, label]) => ({ id, label }));

function stateWord(entityId, value, entity) {
  const d = String(entityId || '').split('.')[0];
  const kind = entity?.capabilities?.kind;
  if (value === null || value === undefined) return '';
  if (d === 'automation') return value === 'on' ? tr("passe à vrai") : tr("passe à faux");
  if (d === 'scene') return value === 'on' ? tr("activée") : tr("désactivée");
  if (d === 'person') return value === 'home' ? tr("arrive à la maison") : 'part';
  if (d === 'binary_sensor') {
    const on = value === 'on';
    if (kind === 'contact') return on ? 'ouvert' : tr("fermé");
    if (kind === 'water_leak') return on ? tr("détecte une fuite") : tr("plus de fuite");
    if (kind === 'smoke') return on ? tr("détecte de la fumée") : tr("plus de fumée");
    if (kind === 'occupancy' || kind === 'presence') return on ? tr("détecte quelqu’un") : tr("plus personne");
    return on ? 'actif' : 'inactif';
  }
  if (value === 'on') return tr("allumé");
  if (value === 'off') return tr("éteint");
  return `« ${value} »`;
}

const SECURITY = {
  disarmed: tr("désactivée"),
  arming: tr("délai de sortie"),
  armed: tr("activée"),
  pending: tr("délai d’entrée : quelqu’un est entré"),
  triggered: tr("DÉCLENCHÉE"),
};

const MODES = { away: 'absent', night: 'nuit' };

const INTEGRATION_STATES = {
  connected: tr("connecté"),
  disconnected: 'injoignable',
  failed: tr("en échec"),
  stopped: tr("désactivé"),
};

const INTEGRATION_NAMES = { zigbee2mqtt: 'Zigbee2MQTT', tuya: tr("Tuya"), matter: tr("Matter"), virtual: tr("Appareils de démo") };

/** { title, text, actor, tone } pour une ligne du journal. */
export function describe(entry, entities = {}) {
  const entity = entry.entity_id ? entities[entry.entity_id] : null;
  const name = entity?.name || entry.name || entry.entity_id || '';
  let text = entry.message || '';
  let tone = 'normal';
  switch (entry.kind) {
    case 'state':
      if (!entry.message) text = stateWord(entry.entity_id, entry.value, entity);
      if (entry.message === 'ne répond plus') tone = 'bad';
      if (['détecte une fuite', 'détecte de la fumée'].includes(text)) tone = 'bad';
      break;
    case 'automation':
      if (text.startsWith('pas lancé')) tone = 'muted';
      if (text.includes('erreur')) tone = 'bad';
      break;
    case 'security': {
      const [phase, mode] = String(entry.value || '').split(':');
      text = `${SECURITY[phase] ?? phase}${mode && phase !== 'disarmed' ? ` (${MODES[mode] ?? mode})` : ''}${entry.message ? ' ' + entry.message : ''}`;
      if (phase === 'triggered') tone = 'bad';
      return { title: tr("Alarme"), text, actor: entry.actor, tone };
    }
    case 'integration': {
      const st = INTEGRATION_STATES[entry.value] ?? entry.value;
      if (entry.value === 'disconnected' || entry.value === 'failed') tone = 'bad';
      return {
        title: INTEGRATION_NAMES[entry.name] ?? entry.name,
        text: `${st}${entry.message ? ` : ${entry.message}` : ''}`,
        actor: null,
        tone,
      };
    }
    case 'system':
      return { title: tr("Heim"), text, actor: entry.actor, tone };
  }
  return { title: name, text, actor: entry.actor, tone };
}

/** « 14:32 », « hier 21:05 », « lun. 12 oct. 08:10 ». */
export function when(iso, now = new Date()) {
  const d = new Date(iso);
  const hm = d.toLocaleTimeString(locale(), { hour: '2-digit', minute: '2-digit' });
  const day = (x) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const diff = Math.round((day(now) - day(d)) / 86400000);
  if (diff === 0) return hm;
  if (diff === 1) return `hier ${hm}`;
  return `${d.toLocaleDateString(locale(), { weekday: 'short', day: 'numeric', month: 'short' })} ${hm}`;
}

/** Regroupe par jour pour l'affichage : [{ day: 'Aujourd’hui', items }]. */
export function byDay(entries, now = new Date()) {
  const out = [];
  for (const e of entries) {
    const d = new Date(e.ts);
    const day = new Date(d.getFullYear(), d.getMonth(), d.getDate());
    const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    const diff = Math.round((today - day) / 86400000);
    const label =
      diff === 0 ? tr("Aujourd’hui") : diff === 1 ? tr("Hier") : d.toLocaleDateString(locale(), { weekday: 'long', day: 'numeric', month: 'long' });
    if (!out.length || out.at(-1).day !== label) out.push({ day: label, items: [] });
    out.at(-1).items.push(e);
  }
  return out;
}
