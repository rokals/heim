import { test } from 'node:test';
import assert from 'node:assert/strict';
import { appearance, choicesFor, defaultCard, resolveAction, ruleMatches, tileValue, valueText } from './tiles.js';

const courrier = (on, enabled = true) => ({
  entity_id: 'automation.courrier',
  name: 'Courrier',
  state: { value: on ? 'on' : 'off', attributes: { enabled, fired: on, fired_at: '2026-10-25T10:00:00Z' }, last_changed: '2026-10-25T10:00:00Z' },
});

test('automatisme : état vrai/faux et facette activé', () => {
  assert.equal(tileValue({ facet: 'state' }, courrier(true)), true);
  assert.equal(valueText({ facet: 'state' }, courrier(false)), 'Faux');
  assert.equal(valueText({ facet: 'enabled' }, courrier(false, false)), 'Désactivé');
});

test('les règles : la première vraie gagne, avec texte et depuis', () => {
  const card = {
    entity_id: 'automation.courrier',
    facet: 'state',
    rules: [{ op: 'eq', value: 'vrai', color: 'red', icon: 'mail', text: 'Courrier depuis {depuis}' }],
  };
  const now = new Date('2026-10-25T12:00:00Z').getTime();
  const a = appearance(card, courrier(true), now);
  assert.equal(a.color, '#e5484d');
  assert.equal(a.icon, 'mail');
  assert.equal(a.text, 'Courrier depuis 2 h');
  const b = appearance(card, courrier(false), now);
  assert.equal(b.color, null);
  assert.equal(b.text, 'Faux');
});

test('comparaisons numériques et mots', () => {
  assert.ok(ruleMatches({ op: 'gt', value: '25' }, 26.5));
  assert.ok(ruleMatches({ op: 'lte', value: '16,5' }, 16.5));
  assert.ok(!ruleMatches({ op: 'gt', value: '25' }, 'on'));
  assert.ok(ruleMatches({ op: 'eq', value: 'ouvert' }, 'on'));
  assert.ok(ruleMatches({ op: 'ne', value: 'faux' }, true));
});

test('règles d’office : température chaude en orange', () => {
  const t = { entity_id: 'sensor.t', capabilities: { kind: 'temperature', unit: '°C' }, state: { value: 27.2 } };
  const a = appearance({ entity_id: 'sensor.t' }, t);
  assert.equal(a.color, '#f76b15');
  assert.equal(a.text, '27,2 °C');
});

test('actions : réarmer, basculer, activer', () => {
  const e = courrier(true);
  assert.deepEqual(resolveAction('rearm', { facet: 'state' }, e), { kind: 'automation', automation: 'courrier', body: { state: 'rearm' } });
  assert.deepEqual(resolveAction('toggle', { facet: 'enabled' }, e).body, { enabled: false });
  assert.deepEqual(resolveAction('toggle', { facet: 'state' }, e).body, { state: false });
  const lamp = { entity_id: 'light.salon', state: { value: 'off' } };
  assert.deepEqual(resolveAction('toggle', {}, lamp), { kind: 'command', entity_id: 'light.salon', action: 'toggle' });
  assert.equal(resolveAction('none', {}, lamp).kind, 'none');
});

test('ajouter : 4 façons pour un automatisme, réglages de départ', () => {
  assert.equal(choicesFor(courrier(false)).length, 4);
  const card = defaultCard(courrier(false), { facet: 'state' });
  assert.equal(card.hold, 'rearm');
  const plug = defaultCard({ entity_id: 'switch.cafe' }, { tap: 'on' });
  assert.equal(plug.tap, 'on');
});
