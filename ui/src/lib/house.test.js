import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildRooms, groupDevices, houseSummary, slug, UNSORTED } from './house.js';
import { clock, duration, formatValue } from './format.js';

const st = (value, extra = {}) => ({ value, available: true, attributes: {}, ...extra });

const entities = [
  { entity_id: 'light.salon', domain: 'light', name: 'Salon/Plafonnier', device_id: '0x01', room: 'Salon', capabilities: { device_name: 'Salon/Plafonnier' }, state: st('on') },
  { entity_id: 'switch.prise_l1', domain: 'switch', name: 'Prise l1', device_id: '0x02', room: 'Cuisine', capabilities: {}, state: st('off') },
  { entity_id: 'sensor.prise_power', domain: 'sensor', name: 'Prise power', device_id: '0x02', capabilities: { kind: 'power', unit: 'W' }, state: st(850) },
  { entity_id: 'binary_sensor.porte_contact', domain: 'binary_sensor', name: 'Porte contact', device_id: '0x03', room: 'Entrée', capabilities: { kind: 'contact', device_name: 'Porte' }, state: st('off') },
  { entity_id: 'sensor.porte_battery', domain: 'sensor', name: 'Porte battery', device_id: '0x03', category: 'diagnostic', capabilities: { kind: 'battery', unit: '%' }, state: st(14) },
  { entity_id: 'sensor.ts0601_temp', domain: 'sensor', name: 'TS0601 temp', device_id: '0x04', capabilities: { kind: 'temperature', device_name: 'TS0601_x7k3' }, state: st(21.4) },
  { entity_id: 'scene.soiree', domain: 'scene', name: 'Soirée', capabilities: {}, state: st('off') },
  { entity_id: 'timer.cafe', domain: 'timer', name: 'Café', room: 'Cuisine', capabilities: {}, state: st('idle') },
];

test('une carte par appareil, la batterie va dans le badge', () => {
  const devices = groupDevices(entities);
  assert.equal(devices.length, 4);
  const porte = devices.find((d) => d.key === '0x03');
  assert.equal(porte.name, 'Porte');
  assert.equal(porte.battery, 14);
  assert.equal(porte.readings.length, 1);
  assert.equal(porte.diagnostics.length, 1);
});

test('rangement par pièce, « à ranger » en dernier', () => {
  const rooms = buildRooms(entities);
  assert.deepEqual(rooms.map((r) => r.name), ['Cuisine', 'Entrée', 'Salon', UNSORTED]);
  const cuisine = rooms[0];
  assert.equal(cuisine.controls.length, 1, 'la prise, sans sa mesure de puissance en double');
  assert.equal(cuisine.timers.length, 1);
  assert.equal(rooms[3].sensors[0].name, 'TS0601_x7k3');
});

test('un thermomètre affiche la température avant l\'humidité', () => {
  const thermo = [
    { entity_id: 'sensor.t_humidity', domain: 'sensor', name: 'h', device_id: 't', capabilities: { kind: 'humidity' }, state: st(48) },
    { entity_id: 'sensor.t_temperature', domain: 'sensor', name: 't', device_id: 't', capabilities: { kind: 'temperature' }, state: st(20) },
  ];
  assert.equal(groupDevices(thermo)[0].readings[0].capabilities.kind, 'temperature');
});

test('un appareil masqué disparaît', () => {
  const hidden = entities.map((e) => (e.device_id === '0x04' ? { ...e, hidden: true } : e));
  assert.ok(!buildRooms(hidden).some((r) => r.name === UNSORTED));
});

test('résumé de la maison', () => {
  const s = houseSummary(entities);
  assert.equal(s.lightsOn.length, 1);
  assert.equal(s.plugsOn.length, 0);
});

test('formats', () => {
  assert.equal(slug('Soirée film !'), 'soiree_film');
  assert.equal(clock(754), '12:34');
  assert.equal(clock(3720), '1:02:00');
  assert.equal(duration(5400), '1 h 30');
  assert.equal(formatValue(entities[3]), 'Fermée');
  assert.equal(formatValue(entities[5]), '21,4 °');
  assert.equal(formatValue(entities[2]), '850 W');
});

test('signal, vu il y a, tableau de bord', async () => {
  const { signalBars, ago, autoTimerId, dashboardFromRooms, missingCards } = await import('./house.js');
  assert.equal(signalBars(200), 4);
  assert.equal(signalBars(10), 0);
  assert.equal(ago(new Date(Date.now() - 5 * 60_000).toISOString()), 'il y a 5 min');
  assert.equal(autoTimerId('switch.demo_prise_cafe'), 'timer.auto_switch_demo_prise_cafe');
  const dash = dashboardFromRooms(entities);
  assert.equal(dash.sections.length, 4);
  assert.deepEqual(missingCards(dash, entities).map((m) => m.entity.entity_id), ['scene.soiree']);
});

test('ordre des pièces de la maison', () => {
  const ents = [
    { entity_id: 'light.a', domain: 'light', name: 'A', room: 'Salon', state: st('on') },
    { entity_id: 'light.b', domain: 'light', name: 'B', room: 'Cuisine', state: st('on') },
    { entity_id: 'light.c', domain: 'light', name: 'C', room: 'Bureau', state: st('on') },
  ];
  assert.deepEqual(buildRooms(ents, ['Salon', 'Cuisine']).map((r) => r.name), ['Salon', 'Cuisine', 'Bureau']);
});
