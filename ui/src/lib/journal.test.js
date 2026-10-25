import { test } from 'node:test';
import assert from 'node:assert/strict';
import { byDay, describe } from './journal.js';

test('les lignes en mots', () => {
  const entities = { 'binary_sensor.porte': { name: 'Porte', capabilities: { kind: 'contact' } } };
  assert.deepEqual(describe({ kind: 'state', entity_id: 'binary_sensor.porte', value: 'on' }, entities), {
    title: 'Porte',
    text: 'ouvert',
    actor: undefined,
    tone: 'normal',
  });
  const lamp = describe({ kind: 'state', entity_id: 'light.salon', name: 'Salon', value: 'on', actor: 'Alex' });
  assert.equal(lamp.text, 'allumé');
  assert.equal(lamp.actor, 'Alex');
  assert.equal(describe({ kind: 'automation', name: 'Soir', message: 'pas lancé : il ne faisait pas nuit' }).tone, 'muted');
  assert.equal(describe({ kind: 'security', value: 'triggered:away' }).tone, 'bad');
  assert.equal(describe({ kind: 'integration', name: 'zigbee2mqtt', value: 'disconnected' }).title, 'Zigbee2MQTT');
});

test('regroupé par jour', () => {
  const now = new Date('2026-10-25T12:00:00');
  const days = byDay(
    [
      { ts: '2026-10-25T11:00:00' },
      { ts: '2026-10-25T08:00:00' },
      { ts: '2026-10-24T22:00:00' },
    ],
    now,
  );
  assert.equal(days.length, 2);
  assert.equal(days[0].day, 'Aujourd’hui');
  assert.equal(days[1].items.length, 1);
});
