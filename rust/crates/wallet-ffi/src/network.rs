//! Typed public-only channel to the trusted launcher's one-shot network workers.
use crate::{INTERNAL, INVALID_ARGUMENT, IO_ERROR, PROTOCOL_ERROR, TIMEOUT, boundary};
use std::{
    fs::File,
    io::{Read, Write},
    os::windows::io::{AsRawHandle, FromRawHandle},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::{FILE_TYPE_PIPE, GetFileType},
    System::{
        Console::{GetStdHandle, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE},
        Pipes::PeekNamedPipe,
        Threading::GetCurrentProcess,
    },
};
struct Channel {
    input: File,
    output: File,
    next: u64,
}
static CHANNEL: OnceLock<Mutex<Option<Channel>>> = OnceLock::new();
fn channel() -> &'static Mutex<Option<Channel>> {
    CHANNEL.get_or_init(Mutex::default)
}
fn duplicate(which: u32) -> Result<File, i32> {
    unsafe {
        let handle = GetStdHandle(which);
        if handle.is_null()
            || handle == INVALID_HANDLE_VALUE
            || GetFileType(handle) != FILE_TYPE_PIPE
        {
            return Err(IO_ERROR);
        }
        let mut copy = std::ptr::null_mut();
        if DuplicateHandle(
            GetCurrentProcess(),
            handle,
            GetCurrentProcess(),
            &mut copy,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        ) == 0
        {
            return Err(IO_ERROR);
        }
        Ok(File::from_raw_handle(copy))
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn w3_network_open() -> i32 {
    boundary(|| {
        wallet_sandbox::verify_offline_token().map_err(|_| -20)?;
        let mut guard = channel().lock().map_err(|_| INTERNAL)?;
        if guard.is_some() {
            return Err(INVALID_ARGUMENT);
        }
        *guard = Some(Channel {
            input: duplicate(STD_INPUT_HANDLE)?,
            output: duplicate(STD_OUTPUT_HANDLE)?,
            next: 1,
        });
        Ok(())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn w3_network_close() -> i32 {
    boundary(|| {
        *channel().lock().map_err(|_| INTERNAL)? = None;
        Ok(())
    })
}

fn validate(opcode: u32, mut bytes: &[u8]) -> Result<(), i32> {
    let mut fields = Vec::new();
    while !bytes.is_empty() {
        if bytes.len() < 2 || fields.len() >= 3 {
            return Err(INVALID_ARGUMENT);
        }
        let len = u16::from_le_bytes(bytes[..2].try_into().unwrap()) as usize;
        if len > 1024 || bytes.len() < len + 2 {
            return Err(INVALID_ARGUMENT);
        }
        fields.push(std::str::from_utf8(&bytes[2..len + 2]).map_err(|_| INVALID_ARGUMENT)?);
        bytes = &bytes[len + 2..];
    }
    let address = |text: &str| {
        chain_evm::Address::parse(text)
            .map(|_| ())
            .map_err(|_| INVALID_ARGUMENT)
    };
    let hash = |text: &str| {
        if text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit()) {
            Ok(())
        } else {
            Err(INVALID_ARGUMENT)
        }
    };
    match (opcode, fields.as_slice()) {
        (18, [account]) => chain_tron::Address::parse(account).map(|_|()).map_err(|_|INVALID_ARGUMENT),
        (19, [sender, recipient, amount]) => {
            chain_tron::Address::parse(sender).map_err(|_|INVALID_ARGUMENT)?;
            chain_tron::Address::parse(recipient).map_err(|_|INVALID_ARGUMENT)?;
            chain_tron::mainnet::amount(amount.strip_prefix("USDT\n").ok_or(INVALID_ARGUMENT)?).map(|_|()).map_err(|_|INVALID_ARGUMENT)
        }
        (20, [raw, txid, summary]) => {
            if raw.len()>1024 { return Err(INVALID_ARGUMENT); }
            let quote=chain_tron::mainnet::Quote::parse(summary).map_err(|_|INVALID_ARGUMENT)?;
            quote.verify_signed(&hex::decode(raw).map_err(|_|INVALID_ARGUMENT)?,txid).map_err(|_|INVALID_ARGUMENT)
        }
        (21, [txid, summary]) => {
            let quote=chain_tron::mainnet::Quote::parse(summary).map_err(|_|INVALID_ARGUMENT)?;
            if hex::encode(quote.tx.txid(chain_tron::mainnet::policy()).map_err(|_|INVALID_ARGUMENT)?)!=*txid {return Err(INVALID_ARGUMENT);}
            Ok(())
        }
        (14, [account]) => chain_solana::Address::wallet(account)
            .map(|_| ())
            .map_err(|_| INVALID_ARGUMENT),
        (15, [sender, recipient, asset_amount]) => {
            chain_solana::Address::wallet(sender).map_err(|_| INVALID_ARGUMENT)?;
            chain_solana::Address::wallet(recipient).map_err(|_| INVALID_ARGUMENT)?;
            let (asset, amount) = asset_amount.split_once('\n').ok_or(INVALID_ARGUMENT)?;
            chain_solana::Asset::parse(asset)
                .and_then(|asset| asset.amount(amount))
                .map(|_| ())
                .map_err(|_| INVALID_ARGUMENT)
        }
        (16, [raw, signature, summary]) => {
            if raw.len() > chain_solana::MAX_TRANSACTION * 2 {
                return Err(INVALID_ARGUMENT);
            }
            let quote = chain_solana::Quote::parse(summary).map_err(|_| INVALID_ARGUMENT)?;
            let signature = chain_solana::signature(signature).map_err(|_| INVALID_ARGUMENT)?;
            let expected = quote
                .transfer
                .signed(&signature)
                .map_err(|_| INVALID_ARGUMENT)?;
            if hex::decode(raw).map_err(|_| INVALID_ARGUMENT)? != expected {
                return Err(INVALID_ARGUMENT);
            }
            Ok(())
        }
        (17, [signature, summary]) => {
            chain_solana::signature(signature).map_err(|_| INVALID_ARGUMENT)?;
            chain_solana::Quote::parse(summary)
                .map(|_| ())
                .map_err(|_| INVALID_ARGUMENT)
        }
        (9, [sender, recipient]) => {
            address(sender)?;
            address(recipient)
        }
        (1 | 5 | 12 | 13, [account]) => address(account),
        (8, [account, ""]) => address(account),
        (7, [account, cursor]) => {
            address(account)?;
            if cursor.is_empty() {
                return Ok(());
            }
            let (end, offset) = cursor.split_once(':').ok_or(INVALID_ARGUMENT)?;
            if end.is_empty()
                || offset.is_empty()
                || end.len() > 16
                || offset.len() > 6
                || !end
                    .bytes()
                    .chain(offset.bytes())
                    .all(|c| c.is_ascii_digit())
                || end.parse::<u64>().map_err(|_| INVALID_ARGUMENT)? > i64::MAX as u64
                || offset.parse::<usize>().map_err(|_| INVALID_ARGUMENT)? > 100_000
            {
                return Err(INVALID_ARGUMENT);
            }
            Ok(())
        }
        (2 | 6, [sender, recipient, amount]) => {
            address(sender)?;
            address(recipient)?;
            let value =
                wallet_core::Amount::parse_units(amount, 18).map_err(|_| INVALID_ARGUMENT)?;
            if value == wallet_core::Amount::ZERO {
                return Err(INVALID_ARGUMENT);
            }
            Ok(())
        }
        (3, [raw, expected]) => {
            hash(expected)?;
            if raw.is_empty() || raw.len() > 1024 {
                return Err(INVALID_ARGUMENT);
            }
            let bytes = hex::decode(raw).map_err(|_| INVALID_ARGUMENT)?;
            chain_evm::decode_signed_transfer(&bytes, chain_evm::bsc::POLICY)
                .map_err(|_| INVALID_ARGUMENT)?;
            if hex::encode(chain_evm::keccak256(&bytes)) != *expected {
                return Err(INVALID_ARGUMENT);
            }
            Ok(())
        }
        (4, [value]) => hash(value),
        _ => Err(INVALID_ARGUMENT),
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn w3_network_exchange(
    _epoch: u64,
    opcode: u32,
    data: *const u8,
    length: u32,
    output: *mut u8,
    capacity: u32,
    written: *mut u32,
) -> i32 {
    boundary(|| {
        if written.is_null() {
            return Err(INVALID_ARGUMENT);
        }
        unsafe {
            *written = 0;
        }
        if data.is_null() || length == 0 || length > 2038 || output.is_null() || capacity != 2048 {
            return Err(INVALID_ARGUMENT);
        }
        unsafe {
            std::ptr::write_bytes(output, 0, capacity as usize);
        }
        let bytes = unsafe { std::slice::from_raw_parts(data, length as usize) };
        validate(opcode, bytes)?;
        let mut guard = channel().lock().map_err(|_| INTERNAL)?;
        let result = guard
            .as_mut()
            .ok_or(IO_ERROR)?
            .exchange(opcode as u8, bytes);
        match result {
            Ok((status, bytes)) => {
                unsafe {
                    std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len());
                    *written = bytes.len() as u32;
                }
                if status != 0 { Err(-40) } else { Ok(()) }
            }
            Err(code) => {
                *guard = None;
                Err(code)
            }
        }
    })
}
impl Channel {
    fn read(&mut self, mut bytes: &mut [u8], deadline: Instant) -> Result<(), i32> {
        while !bytes.is_empty() {
            if Instant::now() >= deadline {
                return Err(TIMEOUT);
            }
            let mut available = 0;
            let ok = unsafe {
                PeekNamedPipe(
                    self.input.as_raw_handle(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &mut available,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(IO_ERROR);
            }
            let count = bytes.len().min(available as usize);
            if count == 0 {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
            self.input
                .read_exact(&mut bytes[..count])
                .map_err(|_| IO_ERROR)?;
            bytes = &mut bytes[count..];
        }
        Ok(())
    }
    fn exchange(&mut self, opcode: u8, body: &[u8]) -> Result<(u16, Vec<u8>), i32> {
        let id = self.next;
        self.next = self.next.checked_add(1).ok_or(PROTOCOL_ERROR)?;
        let mut request = ((body.len() + 10) as u32).to_le_bytes().to_vec();
        request.extend([3, opcode]);
        request.extend(id.to_le_bytes());
        request.extend_from_slice(body);
        self.output.write_all(&request).map_err(|_| IO_ERROR)?;
        self.output.flush().map_err(|_| IO_ERROR)?;
        let deadline = Instant::now() + Duration::from_secs(50);
        let mut prefix = [0; 4];
        self.read(&mut prefix, deadline)?;
        let length = u32::from_le_bytes(prefix) as usize;
        if !(12..=2048).contains(&length) {
            return Err(PROTOCOL_ERROR);
        }
        let mut reply = vec![0; length];
        self.read(&mut reply, deadline)?;
        if reply[0] != 3 || reply[1] != opcode || reply[2..10] != id.to_le_bytes() {
            return Err(PROTOCOL_ERROR);
        }
        let status = u16::from_le_bytes(reply[10..12].try_into().unwrap());
        std::str::from_utf8(&reply[12..]).map_err(|_| PROTOCOL_ERROR)?;
        Ok((status, reply[12..].to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fields(values: &[&str]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for value in values {
            bytes.extend((value.len() as u16).to_le_bytes());
            bytes.extend(value.as_bytes());
        }
        bytes
    }
    #[test]
    fn tron_channel_binds_txid_summary_and_exact_signed_bytes() {
        let f:Vec<_>=include_str!("../../../../tests/fixtures/tron-usdt.txt").lines().filter(|l|!l.starts_with('#')).collect();
        let q=chain_tron::mainnet::Quote {tx:chain_tron::Transaction::decode_signed(&hex::decode(f[4]).unwrap(),chain_tron::mainnet::policy()).unwrap(),bandwidth_fee:400_000};
        let summary=q.summary().unwrap();
        assert!(validate(18,&fields(&[f[0]])).is_ok());
        assert!(validate(19,&fields(&[f[0],f[1],"USDT\n1.234567"])).is_ok());
        assert!(validate(20,&fields(&[f[4],f[3],&summary])).is_ok());
        assert!(validate(21,&fields(&[f[3],&summary])).is_ok());
        assert!(validate(18,&fields(&["wallet password"])).is_err());
        assert!(validate(19,&fields(&[f[0],f[1],"USDT\n0.0000001"])).is_err());
        assert!(validate(20,&fields(&[f[4],f[3],&summary.replace("1.234567","1.234568")])).is_err());
        assert!(validate(20,&fields(&[&format!("{}00",f[4]),f[3],&summary])).is_err());
        assert!(validate(21,&fields(&[&"00".repeat(32),&summary])).is_err());
    }
    #[test]
    fn solana_public_channel_binds_exact_signed_bytes() {
        for line in include_str!("../../../../tests/fixtures/solana-vectors.txt")
            .lines()
            .filter(|l| !l.starts_with('#'))
        {
            let f: Vec<_> = line.split('|').collect();
            let asset = chain_solana::Asset::parse(f[4]).unwrap();
            let quote = chain_solana::Quote {
                transfer: chain_solana::Transfer {
                    sender: chain_solana::Address::parse(f[2]).unwrap(),
                    recipient: chain_solana::Address::parse(f[5]).unwrap(),
                    asset,
                    amount: f[7].parse().unwrap(),
                    blockhash: chain_solana::Address::parse(f[6]).unwrap(),
                    create_recipient_account: f[8] == "1",
                },
                last_valid_height: 1000,
                fee: 5000,
                rent: if f[8] == "1" { 2039280 } else { 0 },
            };
            let summary = quote.summary().unwrap();
            let bytes = hex::decode(f[10]).unwrap();
            let signature = bs58::encode(&bytes[1..65]).into_string();
            assert!(validate(14, &fields(&[f[2]])).is_ok());
            assert!(validate(15, &fields(&[f[2], f[5], &format!("{}\n1", f[4])])).is_ok());
            assert!(validate(16, &fields(&[f[10], &signature, &summary])).is_ok());
            assert!(validate(17, &fields(&[&signature, &summary])).is_ok());
            assert!(
                validate(
                    16,
                    &fields(&[&format!("{}00", f[10]), &signature, &summary])
                )
                .is_err()
            );
            assert!(
                validate(
                    16,
                    &fields(&[
                        f[10],
                        &signature,
                        &summary.replace("solana-mainnet", "devnet")
                    ])
                )
                .is_err()
            );
        }
        assert!(validate(14, &fields(&["0x0000000000000000000000000000000000000001"])).is_err());
        assert!(validate(14, &fields(&["wallet password"])).is_err());
    }
    #[test]
    fn public_channel_rejects_secret_shaped_input_and_unsigned_or_modified_payloads() {
        let account = "0x0000000000000000000000000000000000000001";
        assert!(validate(9, &fields(&[account, account])).is_ok());
        assert!(validate(9, &fields(&[account, "password"])).is_err());
        assert!(validate(9, &fields(&[account, account, "1"])).is_err());
        assert!(validate(7, &fields(&[account, ""])).is_ok());
        assert!(validate(10, &fields(&[account, "100:3"])).is_err());
        assert!(validate(11, &fields(&[account, ""])).is_err());
        assert!(validate(12, &fields(&[account])).is_ok());
        assert!(validate(12, &fields(&["https://example.com"])).is_err());
        assert!(validate(12, &fields(&[account, "extra"])).is_err());
        assert!(validate(13, &fields(&[account])).is_ok());
        assert!(validate(13, &fields(&["../wallet"])).is_err());
        assert!(validate(13, &fields(&[account, "password"])).is_err());
        assert!(validate(10, &fields(&[account, "-1:0"])).is_err());
        assert!(validate(7, &fields(&[account, "100000:3"])).is_ok());
        for cursor in ["wallet-password", "1:2:3", "-1:0", "1:100001"] {
            assert!(validate(7, &fields(&[account, cursor])).is_err());
        }
        assert!(validate(7, &fields(&["wallet-password", ""])).is_err());
        let usdt: Vec<_> = include_str!("../../../../tests/fixtures/usdt-journal.txt")
            .lines()
            .filter(|v| !v.starts_with('#'))
            .collect();
        assert!(validate(3, &fields(&[usdt[1], usdt[0]])).is_ok());
        assert!(validate(5, &fields(&["wallet-password"])).is_err());
        assert!(
            validate(
                6,
                &fields(&[
                    "0x0000000000000000000000000000000000000001",
                    "0x0000000000000000000000000000000000000002",
                    "0"
                ])
            )
            .is_err()
        );
        assert!(validate(1, &fields(&["abandon abandon abandon art"])).is_err());
        assert!(
            validate(
                2,
                &fields(&[
                    "password-for-wallet",
                    "0x0000000000000000000000000000000000000001",
                    "1"
                ])
            )
            .is_err()
        );
        let fixture: Vec<_> = include_str!("../../../../tests/fixtures/bnb-journal.txt")
            .lines()
            .filter(|line| !line.starts_with('#'))
            .collect();
        assert!(validate(3, &fields(&[fixture[1], fixture[0]])).is_ok());
        assert!(validate(3, &fields(&[fixture[1], &"00".repeat(32)])).is_err());
        assert!(validate(3, &fields(&[&format!("{}00", fixture[1]), fixture[0]])).is_err());
        assert!(validate(3, &fields(&["6162616e646f6e", fixture[0]])).is_err());
    }
}
