import test from 'node:test';
import assert from 'node:assert/strict';
import { relativeArtifactPath } from '../src/path-utils.js';
const cases = [
['Windows case', 'C:/Work/app/target', 'c:/work', 'app/target'],
['Backslashes', String.raw`D:\work\app\target`, String.raw`D:\work`, 'app/target'],
['UNC', String.raw`\\server\share\app\target`, '//SERVER/share', 'app/target'],
['Drive root', 'C:/target', 'C:/', 'target'],
['POSIX root', '/work/target', '/', 'work/target'],
['Trailing slash', '/work/target', '/work///', 'target'],
['Same directory', '/work', '/work/', '.'],
['Sibling prefix', '/workspace/target', '/work', '/workspace/target'],
['POSIX case', '/WORK/target', '/work', '/WORK/target'],
];
for (const [label, path, root, expected] of cases) test(label, () => assert.equal(relativeArtifactPath(path, root), expected));
