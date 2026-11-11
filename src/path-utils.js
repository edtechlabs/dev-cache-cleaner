export function relativeArtifactPath(path, root) {
  path = path.replace(/\\/g, '/');
  const normalizedRoot = root.replace(/\\/g, '/').replace(/\/+$/, '');
  const windows = /^[a-z]:/i.test(path) || path.startsWith('//');
  const comparable = windows ? path.toLowerCase() : path;
  const comparableRoot = windows ? normalizedRoot.toLowerCase() : normalizedRoot;
  if (comparable === comparableRoot) return '.';
  const prefix = `${normalizedRoot}/`;
  return comparable.startsWith(windows ? prefix.toLowerCase() : prefix) ? path.slice(prefix.length) : path;
}
