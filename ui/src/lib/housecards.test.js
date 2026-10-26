import { test } from 'node:test';
import assert from 'node:assert/strict';
import { houseEntity, nextSun, resolveHouseAction } from './housecards.js';
import { appearance } from './tiles.js';

const st = (value, extra = {}) => ({ value, available: true, attributes: {}, ...extra });
const ents = [
  { entity_id: 'light.a', domain: 'light', name: 'A', room: 'Salon', state: st('on') },
  { entity_id: 'light.b', domain: 'light', name: 'B', room: 'Cuisine', state: st('off') },
  { entity_id: 'light.c', domain: 'light', name: 'C', room: 'Salon', state: st('on', { available: false }) },
  { entity_id: 'switch.p', domain: 'switch', name: 'P', state: st('on') },
  { entity_id: 'sensor.bat', domain: 'sensor', name: 'Pile', capabilities: { kind: 'battery' }, state: st(9) },
  { entity_id: 'person.alex', domain: 'person', name: 'Alex', state: st('home') },
  { entity_id: 'light.hidden', domain: 'light', name: 'H', hidden: true, state: st('on') },
];

test('toutes les lumières : compte et éteint seulement les allumées joignables', () => {
  const e = houseEntity({ type: 'house', kind: 'lights' }, { entities: ents });
  assert.equal(e.state.value, 1);
  assert.equal(e.state.attributes.text, '1 allumée');
  const r = resolveHouseAction('off', { kind: 'lights' }, e);
  assert.deepEqual(r.commands, [{ entity_id: 'light.a', action: 'turn_off' }]);
  const on = resolveHouseAction('on', { kind: 'lights' }, e);
  assert.deepEqual(on.commands, [{ entity_id: 'light.b', action: 'turn_on' }]);
});

test('je pars : lumières et prises, alarme en option', () => {
  const card = { type: 'house', kind: 'leave', arm: true };
  const e = houseEntity(card, { entities: ents });
  const r = resolveHouseAction('run', card, e);
  assert.equal(r.commands.length, 2);
  assert.equal(r.arm, 'away');
});

test('pièce, piles, personnes', () => {
  const room = houseEntity({ kind: 'room', room: 'Cuisine' }, { entities: ents });
  assert.equal(room.state.attributes.text, 'Éteinte');
  const bat = houseEntity({ kind: 'batteries' }, { entities: ents });
  assert.equal(bat.state.value, 1);
  assert.equal(appearance({ type: 'house', kind: 'batteries' }, bat).color, '#e5484d');
  const people = houseEntity({ kind: 'people' }, { entities: ents });
  assert.equal(people.state.attributes.text, 'Alex');
  assert.equal(appearance({ kind: 'people' }, people).active, true);
});

test('soleil : le prochain événement', () => {
  const sun = { days: [{ rise: '2026-10-26T06:30:00Z', set: '2026-10-26T17:20:00Z' }, null] };
  assert.equal(nextSun(sun, Date.parse('2026-10-26T12:00:00Z')).kind, 'set');
  assert.equal(nextSun(sun, Date.parse('2026-10-26T20:00:00Z')), null);
});
