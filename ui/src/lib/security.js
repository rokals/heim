// Alarme : petites fonctions pures (testées avec `npm test`).

import { tr } from './i18n.js';
export const MODES = {
  away: { label: tr("Absent"), icon: 'shield', hint: tr("Toute la maison est surveillée") },
  night: { label: tr("Nuit"), icon: 'moon', hint: tr("Portes et fenêtres seulement, on peut circuler") },
};

export const ROLES = [
  { id: '', label: tr("Non surveillé") },
  { id: 'perimeter', label: tr("Porte ou fenêtre"), hint: tr("Surveillé en Absent et en Nuit") },
  { id: 'interior', label: tr("Intérieur"), hint: tr("Surveillé seulement en Absent") },
  { id: 'h24', label: '24 h/24', hint: tr("Fumée, fuite, arrachement : toujours") },
];

/** Le rôle qu'on proposerait pour un capteur, d'après ce qu'il mesure. */
export function suggestRole(entity) {
  const kind = entity?.capabilities?.kind ?? '';
  if (kind === 'contact') return 'perimeter';
  if (['occupancy', 'presence', 'motion', 'vibration'].includes(kind)) return 'interior';
  if (['water_leak', 'smoke', 'gas', 'carbon_monoxide', 'tamper'].includes(kind)) return 'h24';
  return '';
}

/** Les capteurs qu'on peut confier à l'alarme : tous les capteurs « ouvert / fermé ». */
export function securityCandidates(entities) {
  return entities
    .filter((e) => e.domain === 'binary_sensor' && (e.category !== 'diagnostic' || e.capabilities?.kind === 'tamper'))
    .sort((a, b) => (a.room || '~').localeCompare(b.room || '~', 'fr') || a.name.localeCompare(b.name, 'fr'));
}

/** Secondes restantes avant `until` (null si pas de délai). */
export function remaining(status, now) {
  if (!status?.until) return null;
  return Math.max(0, Math.ceil((Date.parse(status.until) - now) / 1000));
}

/** Ce que la carte d'alarme doit afficher, en une phrase. */
export function describe(status, now) {
  if (!status) return { tone: 'off', title: tr("Alarme"), line: '' };
  const mode = MODES[status.mode]?.label ?? '';
  const left = remaining(status, now);
  const sirens = status.sirens_until && Date.parse(status.sirens_until) > now;
  switch (status.phase) {
    case 'arming':
      return { tone: 'arming', title: tr("Activation {mode}", { mode }), line: left != null ? tr("dans {left} s", { left }) : '', left };
    case 'armed':
      return { tone: 'armed', title: tr("Protégée · {mode}", { mode }), line: status.bypassed?.length ? tr("{length} capteur(s) ignoré(s)", { length: status.bypassed.length }) : '' };
    case 'pending':
      return {
        tone: 'pending',
        title: status.cause?.name ? tr("{name} : entrée détectée", { name: status.cause.name }) : tr("Entrée détectée"),
        line: left != null ? tr("{left} s pour désactiver", { left }) : '',
        left,
      };
    case 'triggered':
      return { tone: 'alarm', title: tr("Alarme déclenchée"), line: status.cause?.name ?? '', sirens };
    default:
      if (sirens || (status.cause?.h24 && status.cause))
        return { tone: 'alarm', title: tr("Alerte"), line: status.cause?.name ?? '', sirens, h24: true };
      return { tone: 'off', title: tr("Alarme désactivée"), line: '' };
  }
}

/** La configuration permet-elle d'activer l'alarme ? */
export function isConfigured(config) {
  return Boolean(config?.sensors?.some((s) => s.role === 'perimeter' || s.role === 'interior'));
}

export const LOG_LABELS = {
  armed: tr("Activée"),
  arming: tr("Activation"),
  disarmed: tr("Désactivée"),
  pending: tr("Entrée"),
  triggered: tr("Déclenchée"),
  h24: tr("Alerte"),
  offline: tr("Capteur muet"),
  bypassed: tr("Ignoré"),
  wrong_code: tr("Code erroné"),
  snapshot: tr("Photo"),
  test: tr("Essai"),
};
