// PUBLIC fixture entropy only. No network connection, user files or environment secrets.
const {Keypair, PublicKey, Transaction, TransactionMessage, VersionedTransaction, SystemProgram} = require('@solana/web3.js');
const {getAssociatedTokenAddressSync, createAssociatedTokenAccountIdempotentInstruction, createTransferCheckedInstruction} = require('@solana/spl-token');
const {entropyToMnemonic, mnemonicToSeedSync} = require('bip39');
const {derivePath} = require('ed25519-hd-key');
const fs = require('node:fs');
const path = require('node:path');
const mint = new PublicKey('Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB');
const lines=['# PUBLIC entropy|index|address|ATA|asset|recipient|blockhash|units|createATA|messageHex|signedHex'];
for (const entropy of ['00'.repeat(16), '00'.repeat(32), '7f'.repeat(16), require('node:crypto').createHash('sha256').update('Web3Rust PUBLIC Solana fixture 2026-11-18').digest('hex')]) {
  for (const index of [0,1,2]) {
    const seed=mnemonicToSeedSync(entropyToMnemonic(entropy));
    const sender=Keypair.fromSeed(derivePath(`m/44'/501'/${index}'/0'`,seed.toString('hex')).key);
    const recipient=Keypair.fromSeed(Buffer.alloc(32,9)).publicKey;
    const blockhash=new PublicKey(Buffer.alloc(32,7)).toBase58();
    const source=getAssociatedTokenAddressSync(mint,sender.publicKey);
    const destination=getAssociatedTokenAddressSync(mint,recipient);
    for (const [asset,create] of [['SOL',false],['USDT',false],['USDT',true]]) {
      const tx=new Transaction({feePayer:sender.publicKey,recentBlockhash:blockhash});
      if(asset==='SOL') tx.add(SystemProgram.transfer({fromPubkey:sender.publicKey,toPubkey:recipient,lamports:123456789n}));
      else {
        if(create) tx.add(createAssociatedTokenAccountIdempotentInstruction(sender.publicKey,destination,recipient,mint));
        tx.add(createTransferCheckedInstruction(source,mint,destination,sender.publicKey,123456789n,6));
      }
      const message=new TransactionMessage({payerKey:sender.publicKey,recentBlockhash:blockhash,instructions:tx.instructions}).compileToLegacyMessage();
      const signed=new VersionedTransaction(message); signed.sign([sender]);
      lines.push([entropy,index,sender.publicKey.toBase58(),source.toBase58(),asset,recipient.toBase58(),blockhash,'123456789',create?'1':'0',Buffer.from(message.serialize()).toString('hex'),Buffer.from(signed.serialize()).toString('hex')].join('|'));
    }
  }
}
fs.writeFileSync(path.join(__dirname,'../fixtures/solana-vectors.txt'),lines.join('\n')+'\n');
console.log(`Generated ${lines.length-1} PUBLIC Solana vectors`);
