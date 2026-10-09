'use strict';

const fs = require('node:fs');

const version = process.argv[2];
const semanticVersionPattern = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)(?:\.(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*))*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;

if (!version || !semanticVersionPattern.test(version)) {
  throw new Error(`Invalid semantic-release version: ${version || '<missing>'}`);
}

const path = 'Cargo.toml';
const input = fs.readFileSync(path, 'utf8');
let inWorkspacePackage = false;
let replacements = 0;
const output = input.replace(/^([^\n]*)$/gm, (line) => {
  if (/^\s*\[/.test(line)) {
    inWorkspacePackage = /^\s*\[workspace\.package\]\s*$/.test(line);
    return line;
  }
  if (inWorkspacePackage && /^\s*version\s*=/.test(line)) {
    replacements += 1;
    return `version = "${version}"`;
  }
  return line;
});

if (replacements !== 1) {
  throw new Error(`Expected one [workspace.package] version, updated ${replacements}`);
}
fs.writeFileSync(path, output);
