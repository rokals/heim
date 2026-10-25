// Met en mots les lignes du journal : « Lampe salon allumée par Alex ».

const KIND_LABELS = {
  state: 'Appareils',
  automation: 'Automatismes',
  security: 'Alarme',
  integration: 'Connexions',
  system: 'Système',
};

export const KINDS = Object.entries(KIND_LABELS).map(([id, label]) => ({ id, label }));

function stateWord(entityId, value, entity) {
  const d = String(entityId || '').split('.')[0];
  const kind = entity?.capabilities?.kind;
  if (value === null || value === undefined) return '';
  if (d === 'automation') return value === 'on' ? 'passe à vrai' : 'passe à faux';
  if (d === 'scene') return value === 'on' ? 'activée' : 'désactivée';
  if (d === 'person') return value === 'home' ? 'arrive à la maison' : 'part';
  if (d === 'binary_sensor') {
    const on = value === 'on';
    if (kind === 'contact') return on ? 'ouvert' : 'fermé';
    if (kind === 'water_leak') return on ? 'détecte une fuite' : 'plus de fuite';
    if (kind === 'smoke') return on ? 'détecte de la fumée' : 'plus de fumée';
    if (kind === 'occupancy' || kind === 'presence') return on ? 'détecte quelqu’un' : 'plus personne';
    return on ? 'actif' : 'inactif';
  }
  if (value === 'on') return 'allumé';
  if (value === 'off') return 'éteint';
  return `« ${value} »`;
}

const SECURITY = {
  disarmed: 'désactivée',
  arming: 'délai de sortie',
  armed: 'activée',
  pending: 'délai d’entrée : quelqu’un est entré',
  triggered: 'DÉCLENCHÉE',
};

const MODES = { away: 'absent', night: 'nuit' };

const INTEGRATION_STATES = {
  connected: 'connecté',
  disconnected: 'injoignable',
  failed: 'en échec',
  stopped: 'désactivé',
};

const INTEGRATION_NAMES = { zigbee2mqtt: 'Zigbee2MQTT', tuya: 'Tuya', matter: 'Matter', virtual: 'Appareils de démo' };

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
      return { title: 'Alarme', text, actor: entry.actor, tone };
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
      return { title: 'Heim', text, actor: entry.actor, tone };
  }
  return { title: name, text, actor: entry.actor, tone };
}

/** « 14:32 », « hier 21:05 », « lun. 12 oct. 08:10 ». */
export function when(iso, now = new Date()) {
  const d = new Date(iso);
  const hm = d.toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit' });
  const day = (x) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const diff = Math.round((day(now) - day(d)) / 86400000);
  if (diff === 0) return hm;
  if (diff === 1) return `hier ${hm}`;
  return `${d.toLocaleDateString('fr-FR', { weekday: 'short', day: 'numeric', month: 'short' })} ${hm}`;
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
      diff === 0 ? 'Aujourd’hui' : diff === 1 ? 'Hier' : d.toLocaleDateString('fr-FR', { weekday: 'long', day: 'numeric', month: 'long' });
    if (!out.length || out.at(-1).day !== label) out.push({ day: label, items: [] });
    out.at(-1).items.push(e);
  }
  return out;
}
