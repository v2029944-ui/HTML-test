'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const {minisignKeyId, matchesPublicKey} = require('../check-update-signing.cjs');

// Synthetic Minisign-shaped fixture; no actual project keys or signatures.
const pub = 'untrusted comment: minisign public key: A49EF4746FA6E6D7\nRWTX5qZvdPSepDGWg2bz8n6ArQnBMwgQK1geVosorcbLzcunD+KJ+cYe';
const sig = 'untrusted comment: signature from tauri secret key\nRUTX5qZvdPSepPF2zsjvP/L/TQtBLRnWRvaq+4omK1UEugtQp/USrGiI1v762sDy3id3aor7bRC9ETyeot6lMHHrWoZ5hjUM1Qc=\ntrusted comment: timestamp:1791649051';

test('previous signed release uses the same key identity as the updater public key', () => {
  assert.equal(matchesPublicKey(pub, sig), true);
  assert.equal(matchesPublicKey(Buffer.from(pub).toString('base64'), sig), true);
  assert.equal(matchesPublicKey(pub.split('\n')[1], sig), true);
});

test('different signing key is rejected', () => {
  const lines = sig.split('\n');
  const signature = Buffer.from(lines[1], 'base64');
  signature[4] ^= 1;
  lines[1] = signature.toString('base64');
  assert.equal(matchesPublicKey(pub, lines.join('\n')), false);
});

test('malformed or missing key is rejected', () => {
  assert.throws(() => minisignKeyId('', false), /missing/);
  assert.throws(() => minisignKeyId('broken', false), /Invalid/);
  assert.throws(() => minisignKeyId('not-a-signature', true), /Invalid/);
});

test('Tauri .sig artifact wraps the full Minisign signature in Base64', () => {
  const encoded = Buffer.from(sig, 'utf8').toString('base64');
  assert.equal(matchesPublicKey(pub, encoded), true);
  assert.equal(matchesPublicKey(Buffer.from(pub).toString('base64'), encoded), true);
});

test('reject corrupted or unrelated Base64-wrapped Tauri signatures', () => {
  assert.throws(() => minisignKeyId('not-base64!!!', true), /Invalid/);
  assert.throws(() => minisignKeyId(Buffer.from('untrusted comment: invalid\\nnot-signature').toString('base64'), true), /Invalid/);
  const altered = sig.split('\\n');
  const bytes = Buffer.from(altered[1], 'base64');
  bytes[4] ^= 1;
  altered[1] = bytes.toString('base64');
  assert.equal(matchesPublicKey(pub, Buffer.from(altered.join('\\n')).toString('base64')), false);
});
