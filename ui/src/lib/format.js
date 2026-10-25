// Affichage des valeurs de capteurs en français clair.

const number = new Intl.NumberFormat('fr-FR', { maximumFractionDigits: 1 });

const BINARY = {
  contact: ['Ouverte', 'Fermée'],
  occupancy: ['Présence', 'Personne'],
  presence: ['Présence', 'Personne'],
  water_leak: ['Fuite', 'Au sec'],
  smoke: ['Fumée', 'Normal'],
  vibration: ['Vibration', 'Calme'],
  tamper: ['Ouvert', 'Normal'],
  battery_low: ['Pile faible', 'Pile OK'],
};

const LABEL = {
  temperature: 'Température',
  humidity: 'Humidité',
  pressure: 'Pression',
  illuminance: 'Luminosité',
  illuminance_lux: 'Luminosité',
  power: 'Puissance',
  energy: 'Consommation',
  voltage: 'Tension',
  current: 'Courant',
  battery: 'Batterie',
  co2: 'CO₂',
  contact: 'Ouverture',
  occupancy: 'Présence',
  water_leak: 'Fuite',
  device_temperature: 'Température interne',
};

export function kindOf(entity) {
  return entity?.capabilities?.kind || '';
}

export function label(entity) {
  return LABEL[kindOf(entity)] || entity.name;
}

/** « 21,4 °C », « Fermée », « 850 W »… */
export function formatValue(entity) {
  const s = entity?.state;
  if (!s || s.value == null) return '–';
  if (s.available === false) return 'injoignable';
  const kind = kindOf(entity);
  if (entity.domain === 'binary_sensor') {
    const pair = BINARY[kind] || ['Actif', 'Inactif'];
    return s.value === 'on' ? pair[0] : pair[1];
  }
  if (typeof s.value === 'number') {
    const unit = entity.capabilities?.unit || '';
    if (kind === 'temperature' || kind === 'device_temperature') return `${number.format(s.value)} °`;
    return unit ? `${number.format(s.value)} ${unit}` : number.format(s.value);
  }
  return String(s.value).replace(/_/g, ' ');
}

/** Un capteur binaire « actif » mérite d'attirer l'œil (porte ouverte, fuite...). */
export function isAlerting(entity) {
  if (entity.domain !== 'binary_sensor' || entity.state?.value !== 'on') return false;
  return ['contact', 'water_leak', 'smoke', 'tamper', 'gas'].includes(kindOf(entity));
}

export function iconFor(entity) {
  const kind = kindOf(entity);
  if (entity.domain === 'light') return 'bulb';
  if (entity.domain === 'switch') return 'plug';
  if (entity.domain === 'siren') return 'siren';
  if (kind === 'temperature') return 'thermo';
  if (kind === 'humidity') return 'drop';
  if (kind === 'contact') return 'door';
  if (kind === 'occupancy' || kind === 'presence') return 'motion';
  if (kind.startsWith('illuminance')) return 'sun';
  if (kind === 'power' || kind === 'energy') return 'bolt';
  if (kind === 'water_leak') return 'drop';
  return 'sensor';
}

/** 754 -> « 12:34 », 3720 -> « 1:02:00 » */
export function clock(seconds) {
  const s = Math.max(0, Math.round(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const pad = (n) => String(n).padStart(2, '0');
  return h ? `${h}:${pad(m)}:${pad(sec)}` : `${m}:${pad(sec)}`;
}

/** 1800 -> « 30 min », 5400 -> « 1 h 30 » */
export function duration(seconds) {
  const m = Math.round(seconds / 60);
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  const rest = m % 60;
  return rest ? `${h} h ${String(rest).padStart(2, '0')}` : `${h} h`;
}

export function timerRemaining(entity, now) {
  const a = entity.state?.attributes || {};
  const v = entity.state?.value;
  if (v === 'running' && a.ends_at) return Math.max(0, (new Date(a.ends_at).getTime() - now) / 1000);
  if (v === 'paused') return a.remaining_s ?? 0;
  return a.duration_s ?? 0;
}
