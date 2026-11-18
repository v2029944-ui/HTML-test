'use strict';
const fs = require('node:fs');

function minisignKeyId(encoded, isSignature) {
  if (typeof encoded !== 'string' || !encoded.trim()) throw Error('Signing key or signature is missing');
  let input = encoded.trim();
  if (!isSignature && !input.startsWith('untrusted comment:')) {
    const decoded = Buffer.from(input, 'base64');
    if (decoded.length === 42 && decoded.subarray(0, 2).toString('ascii') === 'Ed') {
      return decoded.subarray(2, 10).toString('hex');
    }
    input = decoded.toString('utf8').trim();
  }
  const lines = input.split(/\r?\n/);
  if (!lines[0].startsWith('untrusted comment:') || !lines[1]) {
    throw Error('Invalid Minisign key or signature format');
  }
  const bytes = Buffer.from(lines[1].trim(), 'base64');
  const type = bytes.subarray(0, 2).toString('ascii');
  if ((isSignature && bytes.length !== 74) ||
      (!isSignature && bytes.length !== 42) ||
      (isSignature ? type !== 'ED' && type !== 'Ed' : type !== 'Ed')) {
    throw Error('Invalid Minisign key or signature payload');
  }
  return bytes.subarray(2, 10).toString('hex');
}

function matchesPublicKey(publicKey, signature) {
  return minisignKeyId(publicKey, false) === minisignKeyId(signature, true);
}

if (require.main === module) {
  try {
    const path = process.argv[2];
    if (!path || !process.env.ANALIZ_UPDATER_PUBLIC_KEY) throw Error('Public key or signature path missing');
    if (!matchesPublicKey(process.env.ANALIZ_UPDATER_PUBLIC_KEY, fs.readFileSync(path, 'utf8'))) {
      throw Error('Public updater key does not match the signed artifact');
    }
    process.stdout.write('Updater key identity matches the signature.\n');
  } catch (error) {
    console.error('Updater signing-key check failed: ' + error.message);
    process.exitCode = 1;
  }
}

module.exports = { minisignKeyId, matchesPublicKey };
