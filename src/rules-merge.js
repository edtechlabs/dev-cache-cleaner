export function artifactRuleKey(rule) {
  return [rule.directoryName, rule.artifactKind, ...(rule.requiresOutside || []),
    ...(rule.requiresOutsideAny || []), ...(rule.requiresInside || []),
    ...(rule.locations || [])].join('|');
}

// Stored rules predate newer built-in rules. Append the built-in rules the
// stored set does not have yet, keeping the user's own rules and order.
export function mergeDefaultRules(stored, defaults) {
  const known = new Set(stored.map(artifactRuleKey));
  return [...stored, ...defaults.filter((rule) => !known.has(artifactRuleKey(rule)))];
}
