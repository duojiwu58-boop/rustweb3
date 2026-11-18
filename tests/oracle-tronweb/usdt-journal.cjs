// Fixed PUBLIC zero-entropy wallet. Never fund this test vector.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
let networkAttempts = 0;
const deny = () => { networkAttempts++; throw Error('Offline oracle'); };
for (const name of ['node:http', 'node:https']) { const module = require(name); module.request = deny; module.get = deny; }
require('node:net').Socket.prototype.connect = deny;
require('node:tls').connect = deny;
globalThis.fetch = deny;
const ethers = require('ethers');
assert.equal(ethers.version, '6.17.0');
(async () => {
  const phrase = ethers.Mnemonic.fromEntropy('0x' + '00'.repeat(32)).phrase;
  const wallet = ethers.HDNodeWallet.fromPhrase(phrase, '', "m/44'/60'/0'/0/0");
  const data = new ethers.Interface(['function transfer(address,uint256)']).encodeFunctionData('transfer', [wallet.address, 1234567890123456789n]);
  const signed = await wallet.signTransaction({ type: 0, chainId: 56, nonce: 7, gasPrice: 1000000000n, gasLimit: 60000n, to: '0x55d398326f99059fF775485246999027B3197955', value: 0n, data });
  const fixture = '# PUBLIC zero entropy BSC USDT journal vector. NEVER FUND. ethers 6.17.0\n' + ethers.keccak256(signed).slice(2) + '\n' + signed.slice(2) + '\n';
  const target = path.join(__dirname, '../fixtures/usdt-journal.txt');
  if (process.argv.includes('--write')) fs.writeFileSync(target, fixture);
  else assert.equal(fs.readFileSync(target, 'utf8'), fixture);
  assert.equal(networkAttempts, 0);
  console.log('PASS: USDT public journal signed bytes/hash; zero network attempts');
})().catch(error => { console.error(error); process.exitCode = 1; });
