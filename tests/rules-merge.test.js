import test from 'node:test';
import assert from 'node:assert/strict';
import { mergeDefaultRules } from '../src/rules-merge.js';
import config from '../src/project-artifact-rules.json' with { type: 'json' };

test('merge appends missing built-in rules and keeps custom ones', () => {
  const custom = { directoryName: 'custom', artifactKind: 'Mine', locations: ['any'] };
  const old = config.rules.slice(0, 5).concat(custom);
  const merged = mergeDefaultRules(old, config.rules);
  assert.equal(merged.length, config.rules.length + 1);
  assert.equal(merged[5], custom);
  assert.ok(merged.some((r) => r.artifactKind === '.NET build'));
});

test('merge is idempotent and respects edited rules', () => {
  const once = mergeDefaultRules(config.rules, config.rules);
  assert.equal(once.length, config.rules.length);
  const disabled = config.rules.map((r, i) => (i === 0 ? { ...r, enabled: false } : r));
  assert.equal(mergeDefaultRules(disabled, config.rules).length, config.rules.length);
});
