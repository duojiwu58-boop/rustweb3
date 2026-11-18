// PUBLIC test entropy only; no arguments accepting a seed, password or mnemonic.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
let networkAttempts = 0;
const deny = () => { networkAttempts++; throw Error('Offline oracle'); };
for (const name of ['node:http', 'node:https']) {
  const module = require(name); module.request = deny; module.get = deny;
}
require('node:net').Socket.prototype.connect = deny;
require('node:tls').connect = deny;
globalThis.fetch = deny;
const ethers = require('ethers');
const utils = require('tronweb/utils').default;
assert.equal(ethers.version, '6.17.0');
const rows = ['00', '7f', 'ff'].map(byte => {
  const entropy = byte.repeat(32);
  const phrase = ethers.Mnemonic.fromEntropy('0x' + entropy).phrase;
  const bsc = ethers.HDNodeWallet.fromPhrase(phrase, '', "m/44'/60'/0'/0/0");
  const tron = ethers.HDNodeWallet.fromPhrase(phrase, '', "m/44'/195'/0'/0/0");
  return [entropy, bsc.address, utils.crypto.pkToAddress(tron.privateKey.slice(2))].join('|');
});
const fixture = '# PUBLIC TEST ENTROPY. NEVER FUND. ethers 6.17.0 / TronWeb 6.5.1; empty passphrase.\n' + rows.join('\n') + '\n';
const target = path.join(__dirname, '../fixtures/hd-accounts.txt');
if (process.argv.includes('--write')) fs.writeFileSync(target, fixture);
else assert.equal(fs.readFileSync(target, 'utf8'), fixture);
assert.equal(networkAttempts, 0);
console.log('PASS: 3 public 24-word HD vectors; BSC/TRON; zero network attempts');
