// Alarme : petites fonctions pures (testées avec `npm test`).

export const MODES = {
  away: { label: 'Absent', icon: 'shield', hint: 'Toute la maison est surveillée' },
  night: { label: 'Nuit', icon: 'moon', hint: 'Portes et fenêtres seulement, on peut circuler' },
};

export const ROLES = [
  { id: '', label: 'Non surveillé' },
  { id: 'perimeter', label: 'Porte ou fenêtre', hint: 'Surveillé en Absent et en Nuit' },
  { id: 'interior', label: 'Intérieur', hint: 'Surveillé seulement en Absent' },
  { id: 'h24', label: '24 h/24', hint: 'Fumée, fuite, arrachement : toujours' },
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
  if (!status) return { tone: 'off', title: 'Alarme', line: '' };
  const mode = MODES[status.mode]?.label ?? '';
  const left = remaining(status, now);
  const sirens = status.sirens_until && Date.parse(status.sirens_until) > now;
  switch (status.phase) {
    case 'arming':
      return { tone: 'arming', title: `Activation ${mode}`, line: left != null ? `dans ${left} s` : '', left };
    case 'armed':
      return { tone: 'armed', title: `Protégée · ${mode}`, line: status.bypassed?.length ? `${status.bypassed.length} capteur(s) ignoré(s)` : '' };
    case 'pending':
      return {
        tone: 'pending',
        title: status.cause?.name ? `${status.cause.name} : entrée détectée` : 'Entrée détectée',
        line: left != null ? `${left} s pour désactiver` : '',
        left,
      };
    case 'triggered':
      return { tone: 'alarm', title: 'Alarme déclenchée', line: status.cause?.name ?? '', sirens };
    default:
      if (sirens || (status.cause?.h24 && status.cause))
        return { tone: 'alarm', title: 'Alerte', line: status.cause?.name ?? '', sirens, h24: true };
      return { tone: 'off', title: 'Alarme désactivée', line: '' };
  }
}

/** La configuration permet-elle d'activer l'alarme ? */
export function isConfigured(config) {
  return Boolean(config?.sensors?.some((s) => s.role === 'perimeter' || s.role === 'interior'));
}

export const LOG_LABELS = {
  armed: 'Activée',
  arming: 'Activation',
  disarmed: 'Désactivée',
  pending: 'Entrée',
  triggered: 'Déclenchée',
  h24: 'Alerte',
  offline: 'Capteur muet',
  bypassed: 'Ignoré',
  wrong_code: 'Code erroné',
  snapshot: 'Photo',
  test: 'Essai',
};
