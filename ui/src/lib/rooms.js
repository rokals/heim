// Pièces courantes, avec une icône, pour aller vite.
export const ROOM_PRESETS = [
  { name: 'Salon', icon: 'sofa' },
  { name: 'Cuisine', icon: 'kitchen' },
  { name: 'Chambre', icon: 'bed' },
  { name: 'Chambre d’amis', icon: 'bed' },
  { name: 'Chambre enfant', icon: 'child' },
  { name: 'Salle de bain', icon: 'bath' },
  { name: 'Toilettes', icon: 'bath' },
  { name: 'Entrée', icon: 'door' },
  { name: 'Couloir', icon: 'door' },
  { name: 'Bureau', icon: 'desk' },
  { name: 'Buanderie', icon: 'laundry' },
  { name: 'Garage', icon: 'car' },
  { name: 'Jardin', icon: 'tree' },
  { name: 'Terrasse', icon: 'sun' },
  { name: 'Escalier', icon: 'stairs' },
];

export const ROOM_ICONS = ['house', 'sofa', 'kitchen', 'bed', 'child', 'bath', 'door', 'desk', 'laundry', 'car', 'tree', 'sun', 'stairs', 'bulb'];

/** L'icône d'une pièce : la sienne, sinon celle d'une pièce courante au nom proche. */
export function roomIcon(room) {
  if (room?.icon) return room.icon;
  const n = (room?.name ?? '').toLowerCase();
  const hit = ROOM_PRESETS.find((p) => n.startsWith(p.name.toLowerCase().split(' ')[0]));
  return hit?.icon ?? 'house';
}

export const FLOOR_PRESETS = ['Rez-de-chaussée', 'Étage', 'Sous-sol', 'Combles', 'Extérieur'];
