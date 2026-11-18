// Offline, public fixture key only. Never fund these addresses.
const fs = require('node:fs');
const assert = require('node:assert/strict');
const deny = () => { throw Error('Network forbidden'); };
for (const name of ['node:http','node:https']) { const m=require(name); m.request=deny; m.get=deny; }
require('node:net').Socket.prototype.connect=deny;
require('node:tls').connect=deny; globalThis.fetch=deny;
const u=require('tronweb/utils').default;
const key='2d8f68944bdbfbc0769542fba8fc2d2a3de67393334471624364c7006da2aa54';
const owner=u.crypto.pkToAddress(key),recipient='TW1dU4L3eNm7Lw8WvieLKEHpXWAussRG9Z';
const data='a9059cbb'+u.abi.encodeParams(['address','uint256'],[recipient,'1234567']).slice(2);
const tx={visible:false,raw_data:{ref_block_bytes:'7b3b',ref_block_hash:'b21ace8d6ac20e7e',timestamp:1800000000000,expiration:1800000060000,fee_limit:30000000,
  contract:[{type:'TriggerSmartContract',parameter:{type_url:'type.googleapis.com/protocol.TriggerSmartContract',value:{owner_address:u.address.toHex(owner),contract_address:u.address.toHex('TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t'),data}}}]}};
const pb=u.transaction.txJsonToPb(tx);
tx.raw_data_hex=u.transaction.txPbToRawDataHex(pb).toLowerCase();
tx.txID=u.transaction.txPbToTxID(pb).replace(/^0x/,'').toLowerCase();
const sig=u.crypto.signTransaction(key,tx).signature[0];
const normalized=sig.slice(0,-2)+(parseInt(sig.slice(-2),16)-27).toString(16).padStart(2,'0');
pb.addSignature(Buffer.from(normalized,'hex'));
const fixture='# PUBLIC fixture, never fund. TronWeb 6.5.1, zero network.\n'+[owner,recipient,tx.raw_data_hex,tx.txID,Buffer.from(pb.serializeBinary()).toString('hex')].join('\n')+'\n';
const path=require('node:path').join(__dirname,'../fixtures/tron-usdt.txt');
if(process.argv.includes('--write')) fs.writeFileSync(path,fixture); else assert.equal(fs.readFileSync(path,'utf8'),fixture);
console.log('PASS: Tron USDT independent ABI/protobuf/hash/signature fixture');
