// Réglages d'appareils : noms en français, rangement, conversions. Fonctions pures (npm test).

const LABELS = {
  power_on_behavior: 'Après une coupure de courant',
  power_on_state: 'Après une coupure de courant',
  power_outage_memory: 'Se souvenir de l’état après une coupure',
  relay_status: 'Après une coupure de courant',
  restore_mode: 'Après une coupure de courant',
  child_lock: 'Verrouillage enfant',
  indicator_mode: 'Voyant lumineux',
  led_indication: 'Voyant lumineux',
  led_disabled_night: 'Voyant éteint la nuit',
  light_mode: 'Voyant lumineux',
  backlight_mode: 'Rétroéclairage',
  effect: 'Effet',
  color_temp_startup: 'Température au rallumage',
  color_temp: 'Température',
  brightness: 'Luminosité',
  on_level: 'Luminosité à l’allumage',
  'level_config.on_level': 'Luminosité à l’allumage',
  'level_config.current_level_startup': 'Luminosité au rallumage',
  'level_config.on_off_transition_time': 'Fondu allumer / éteindre',
  'level_config.on_transition_time': 'Fondu à l’allumage',
  'level_config.off_transition_time': 'Fondu à l’extinction',
  'level_config.execute_if_off': 'Accepter les réglages même éteinte',
  'color_options.execute_if_off': 'Accepter la couleur même éteinte',
  execute_if_off: 'Accepter les réglages même éteinte',
  switch_type: 'Type d’interrupteur mural',
  operation_mode: 'Mode de fonctionnement',
  sensitivity: 'Sensibilité',
  motion_sensitivity: 'Sensibilité au mouvement',
  occupancy_timeout: 'Délai avant « personne »',
  illuminance_threshold: 'Seuil de luminosité',
  temperature_calibration: 'Correction de la température',
  humidity_calibration: 'Correction de l’humidité',
  countdown: 'Minuterie intégrée',
  countdown_1: 'Minuterie intégrée',
  overload_protection: 'Protection surcharge',
  state: 'Allumé',
  switch_1: 'Interrupteur 1',
  switch_2: 'Interrupteur 2',
  switch_3: 'Interrupteur 3',
  switch_usb1: 'USB',
  warning: 'Sirène',
  volume: 'Volume',
  alarm: 'Alarme',
  melody: 'Mélodie',
  duration: 'Durée',
  identify: 'Identifier',
};

const VALUES = {
  coolest: 'Le plus froid',
  warmest: 'Le plus chaud',
  neutral: 'Neutre',
  off: 'Éteint',
  on: 'Allumé',
  OFF: 'Éteint',
  ON: 'Allumé',
  toggle: 'Inverser',
  previous: 'Comme avant',
  last: 'Comme avant',
  memory: 'Comme avant',
  restore: 'Comme avant',
  power_off: 'Éteint',
  power_on: 'Allumé',
  blink: 'Clignoter',
  breathe: 'Respirer',
  okay: 'Confirmer',
  channel_change: 'Changement de canal',
  finish_effect: 'Finir l’effet',
  stop_effect: 'Arrêter',
  colorloop: 'Arc-en-ciel',
  stop_colorloop: 'Arrêter l’arc-en-ciel',
  low: 'Faible',
  medium: 'Moyenne',
  high: 'Haute',
  very_high: 'Très haute',
  none: 'Aucun',
  relay: 'Suit le relais',
  pos: 'Inversé',
  toggle_switch: 'Interrupteur à bascule',
  momentary: 'Bouton poussoir',
  momentary_switch: 'Bouton poussoir',
  state: 'Interrupteur',
  true: 'Oui',
  false: 'Non',
  LOCK: 'Verrouillé',
  UNLOCK: 'Déverrouillé',
};

/** « temperature_calibration » -> « Temperature calibration » (dernier recours). */
function humanize(s) {
  const t = String(s).replace(/[._]/g, ' ').replace(/\s+/g, ' ').trim();
  return t.charAt(0).toUpperCase() + t.slice(1);
}

export function controlLabel(c) {
  return LABELS[c.key] ?? LABELS[c.name] ?? humanize(c.label || c.name || c.key);
}

// Les descriptions de Zigbee2MQTT sont en anglais : on ne les montre que pour les
// réglages qu'on n'a pas su nommer en français.
export function showDescription(c) {
  return Boolean(c.description) && !(LABELS[c.key] ?? LABELS[c.name]);
}

export function valueLabel(v) {
  return VALUES[String(v)] ?? humanize(v);
}

/** Valeur d'un réglage dans l'état reçu (« a.b » : imbriqué). */
export function valueAt(values, key) {
  return key.split('.').reduce((o, k) => (o == null ? undefined : o[k]), values);
}

/**
 * Range les réglages : la lumière (luminosité, température, couleur, effet), les autres
 * commandes, puis la configuration.
 */
export function groupControls(controls = []) {
  const light = {};
  const main = [];
  const settings = [];
  for (const c of controls) {
    if (c.group === 'light' && ['state', 'brightness', 'color_temp', 'color'].includes(c.name === 'color_xy' || c.name === 'color_hs' ? 'color' : c.name) && !c.endpoint) {
      light[c.kind === 'color' ? 'color' : c.name] = c;
    } else if (c.name === 'effect' && c.kind === 'enum') {
      light.effect = c;
    } else if ((c.group === 'light' || c.group === 'switch') && c.name === 'state') {
      main.push(c);
    } else if (c.category !== 'diagnostic') {
      settings.push(c);
    }
  }
  return { light: Object.keys(light).length ? light : null, main, settings };
}

/** Le réglage « après coupure de courant », s'il existe (pour le mettre en avant). */
export function isPowerOn(c) {
  return ['power_on_behavior', 'power_on_state', 'power_outage_memory', 'relay_status', 'restore_mode'].includes(c.name);
}

/** 0..254 -> « 52 % » */
export function percent(v, max = 254) {
  return `${Math.round((Number(v) / max) * 100)} %`;
}

/** Mireds -> Kelvin (153 -> 6536 K). */
export function kelvin(mireds) {
  return Math.round(1_000_000 / mireds / 50) * 50;
}

/** {x, y} (CIE) -> « #rrggbb », pour montrer la couleur actuelle d'une lampe. */
export function xyToHex({ x, y }, brightness = 254) {
  if (!x || !y) return null;
  const z = 1 - x - y;
  const Y = brightness / 254;
  const X = (Y / y) * x;
  const Z = (Y / y) * z;
  let r = X * 1.656492 - Y * 0.354851 - Z * 0.255038;
  let g = -X * 0.707196 + Y * 1.655397 + Z * 0.036152;
  let b = X * 0.051713 - Y * 0.121364 + Z * 1.01153;
  const m = Math.max(r, g, b, 1e-6);
  [r, g, b] = [r, g, b].map((c) => Math.max(0, c / m));
  const gamma = (c) => (c <= 0.0031308 ? 12.92 * c : 1.055 * Math.pow(c, 1 / 2.4) - 0.055);
  return '#' + [r, g, b].map((c) => Math.round(Math.min(1, gamma(c)) * 255).toString(16).padStart(2, '0')).join('');
}

/** {hue, saturation} -> « #rrggbb » */
export function hsToHex({ hue, saturation }) {
  if (hue == null) return null;
  const s = (saturation ?? 100) / 100;
  const f = (n) => {
    const k = (n + hue / 60) % 6;
    return 1 - s * Math.max(0, Math.min(k, 4 - k, 1));
  };
  return '#' + [f(5), f(3), f(1)].map((c) => Math.round(c * 255).toString(16).padStart(2, '0')).join('');
}

export const SWATCHES = ['#ffb35c', '#ff6b4a', '#ff3b6b', '#c86bff', '#5b7cff', '#3ec6ff', '#4be0a8', '#d8ff5c'];

export const FADES = [
  { value: 0, label: 'Instantané' },
  { value: 0.5, label: '½ s' },
  { value: 1, label: '1 s' },
  { value: 2, label: '2 s' },
  { value: 5, label: '5 s' },
];
