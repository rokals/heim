// Les petites courbes des cartes : une demande par capteur toutes les 5 minutes au plus,
// partagée entre toutes les cartes qui montrent le même capteur.
import { api } from './store.svelte.js';

const REFRESH_MS = 5 * 60_000;
export const sparks = $state({});
const fetchedAt = new Map();

export function wantSpark(entityId, hours = 24) {
  const key = `${entityId}|${hours}`;
  const at = fetchedAt.get(key) ?? 0;
  if (Date.now() - at < REFRESH_MS) return;
  fetchedAt.set(key, Date.now());
  api(`/history/${encodeURIComponent(entityId)}?hours=${hours}&points=48`)
    .then((p) => (sparks[key] = Array.isArray(p) ? p : []))
    .catch(() => fetchedAt.delete(key));
}

export function sparkOf(entityId, hours = 24) {
  return sparks[`${entityId}|${hours}`] ?? null;
}
