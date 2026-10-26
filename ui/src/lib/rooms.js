// Pièces courantes, avec une icône, pour aller vite.
import { tr } from './i18n.js';
export const ROOM_PRESETS = [
  { name: tr("Salon"), icon: 'sofa' },
  { name: tr("Cuisine"), icon: 'kitchen' },
  { name: tr("Chambre"), icon: 'bed' },
  { name: tr("Chambre d’amis"), icon: 'bed' },
  { name: tr("Chambre enfant"), icon: 'child' },
  { name: tr("Salle de bain"), icon: 'bath' },
  { name: tr("Toilettes"), icon: 'bath' },
  { name: tr("Entrée"), icon: 'door' },
  { name: tr("Couloir"), icon: 'door' },
  { name: tr("Bureau"), icon: 'desk' },
  { name: tr("Buanderie"), icon: 'laundry' },
  { name: tr("Garage"), icon: 'car' },
  { name: tr("Jardin"), icon: 'tree' },
  { name: tr("Terrasse"), icon: 'sun' },
  { name: tr("Escalier"), icon: 'stairs' },
];

export const ROOM_ICONS = ['house', 'sofa', 'kitchen', 'bed', 'child', 'bath', 'door', 'desk', 'laundry', 'car', 'tree', 'sun', 'stairs', 'bulb'];

/** L'icône d'une pièce : la sienne, sinon celle d'une pièce courante au nom proche. */
export function roomIcon(room) {
  if (room?.icon) return room.icon;
  const n = (room?.name ?? '').toLowerCase();
  const hit = ROOM_PRESETS.find((p) => n.startsWith(p.name.toLowerCase().split(' ')[0]));
  return hit?.icon ?? 'house';
}

export const FLOOR_PRESETS = [tr("Rez-de-chaussée"), tr("Étage"), 'Sous-sol', tr("Combles"), tr("Extérieur")];
