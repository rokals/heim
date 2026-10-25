import test from 'node:test';
import assert from 'node:assert/strict';
import { describe, isConfigured, remaining, securityCandidates, suggestRole } from './security.js';

test('rôle proposé selon le type de capteur', () => {
  assert.equal(suggestRole({ capabilities: { kind: 'contact' } }), 'perimeter');
  assert.equal(suggestRole({ capabilities: { kind: 'occupancy' } }), 'interior');
  assert.equal(suggestRole({ capabilities: { kind: 'water_leak' } }), 'h24');
  assert.equal(suggestRole({ capabilities: { kind: 'temperature' } }), '');
});

test('candidats : capteurs binaires, plus l’arrachement même en diagnostic', () => {
  const list = securityCandidates([
    { entity_id: 'binary_sensor.a', domain: 'binary_sensor', name: 'A', capabilities: { kind: 'contact' } },
    { entity_id: 'binary_sensor.t', domain: 'binary_sensor', name: 'T', category: 'diagnostic', capabilities: { kind: 'tamper' } },
    { entity_id: 'binary_sensor.b', domain: 'binary_sensor', name: 'B', category: 'diagnostic', capabilities: { kind: 'battery_low' } },
    { entity_id: 'sensor.x', domain: 'sensor', name: 'X' },
  ]);
  assert.deepEqual(list.map((e) => e.entity_id), ['binary_sensor.a', 'binary_sensor.t']);
});

test('décompte et phrases de la carte', () => {
  const now = Date.parse('2026-10-24T10:00:00Z');
  const arming = { phase: 'arming', mode: 'away', until: '2026-10-24T10:00:42Z' };
  assert.equal(remaining(arming, now), 42);
  assert.equal(describe(arming, now).title, 'Activation Absent');
  assert.equal(describe({ phase: 'armed', mode: 'night', bypassed: [] }, now).title, 'Protégée · Nuit');
  const h24 = { phase: 'disarmed', cause: { name: 'Fuite cuisine', h24: true }, sirens_until: '2026-10-24T10:03:00Z' };
  assert.equal(describe(h24, now).tone, 'alarm');
  assert.equal(describe({ phase: 'disarmed' }, now).tone, 'off');
});

test('alarme configurée seulement avec un capteur périmètre ou intérieur', () => {
  assert.equal(isConfigured({ sensors: [{ role: 'h24' }] }), false);
  assert.equal(isConfigured({ sensors: [{ role: 'perimeter' }] }), true);
  assert.equal(isConfigured(null), false);
});
