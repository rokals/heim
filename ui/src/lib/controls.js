// Réglages d'appareils : noms en français, rangement, conversions. Fonctions pures (npm test).

import { tr } from './i18n.js';
const LABELS = {
  power_on_behavior: tr("Après une coupure de courant"),
  power_on_state: tr("Après une coupure de courant"),
  power_outage_memory: tr("Se souvenir de l’état après une coupure"),
  relay_status: tr("Après une coupure de courant"),
  restore_mode: tr("Après une coupure de courant"),
  child_lock: tr("Verrouillage enfant"),
  indicator_mode: tr("Voyant lumineux"),
  led_indication: tr("Voyant lumineux"),
  led_disabled_night: tr("Voyant éteint la nuit"),
  light_mode: tr("Voyant lumineux"),
  backlight_mode: tr("Rétroéclairage"),
  effect: tr("Effet"),
  color_temp_startup: tr("Température au rallumage"),
  color_temp: tr("Température"),
  brightness: tr("Luminosité"),
  on_level: tr("Luminosité à l’allumage"),
  'level_config.on_level': tr("Luminosité à l’allumage"),
  'level_config.current_level_startup': tr("Luminosité au rallumage"),
  'level_config.on_off_transition_time': tr("Fondu allumer / éteindre"),
  'level_config.on_transition_time': tr("Fondu à l’allumage"),
  'level_config.off_transition_time': tr("Fondu à l’extinction"),
  'level_config.execute_if_off': tr("Accepter les réglages même éteinte"),
  'color_options.execute_if_off': tr("Accepter la couleur même éteinte"),
  execute_if_off: tr("Accepter les réglages même éteinte"),
  switch_type: tr("Type d’interrupteur mural"),
  operation_mode: tr("Mode de fonctionnement"),
  sensitivity: tr("Sensibilité"),
  motion_sensitivity: tr("Sensibilité au mouvement"),
  occupancy_timeout: tr("Délai avant « personne »"),
  illuminance_threshold: tr("Seuil de luminosité"),
  temperature_calibration: tr("Correction de la température"),
  humidity_calibration: tr("Correction de l’humidité"),
  countdown: tr("Minuterie intégrée"),
  countdown_1: tr("Minuterie intégrée"),
  overload_protection: tr("Protection surcharge"),
  state: 'Allumé',
  switch_1: tr("Interrupteur 1"),
  switch_2: tr("Interrupteur 2"),
  switch_3: tr("Interrupteur 3"),
  switch_usb1: 'USB',
  warning: tr("Sirène"),
  volume: tr("Volume"),
  alarm: tr("Alarme"),
  melody: tr("Mélodie"),
  duration: tr("Durée"),
  identify: tr("Identifier"),
};

const VALUES = {
  coolest: tr("Le plus froid"),
  warmest: tr("Le plus chaud"),
  neutral: tr("Neutre"),
  off: tr("Éteint"),
  on: tr("Allumé"),
  OFF: tr("Éteint"),
  ON: tr("Allumé"),
  toggle: tr("Inverser"),
  previous: tr("Comme avant"),
  last: tr("Comme avant"),
  memory: tr("Comme avant"),
  restore: tr("Comme avant"),
  power_off: tr("Éteint"),
  power_on: tr("Allumé"),
  blink: tr("Clignoter"),
  breathe: tr("Respirer"),
  okay: tr("Confirmer"),
  channel_change: tr("Changement de canal"),
  finish_effect: tr("Finir l’effet"),
  stop_effect: tr("Arrêter"),
  colorloop: 'Arc-en-ciel',
  stop_colorloop: tr("Arrêter l’arc-en-ciel"),
  low: tr("Faible"),
  medium: tr("Moyenne"),
  high: tr("Haute"),
  very_high: tr("Très haute"),
  none: tr("Aucun"),
  relay: tr("Suit le relais"),
  pos: tr("Inversé"),
  toggle_switch: tr("Interrupteur à bascule"),
  momentary: tr("Bouton poussoir"),
  momentary_switch: tr("Bouton poussoir"),
  state: 'Interrupteur',
  true: tr("Oui"),
  false: tr("Non"),
  LOCK: tr("Verrouillé"),
  UNLOCK: tr("Déverrouillé"),
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
  { value: 0, label: tr("Instantané") },
  { value: 0.5, label: '½ s' },
  { value: 1, label: '1 s' },
  { value: 2, label: '2 s' },
  { value: 5, label: '5 s' },
];
