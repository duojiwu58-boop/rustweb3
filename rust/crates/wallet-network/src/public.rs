//! One bounded public request per contained worker. No persistent peer can fill stdin.
use serde_json::{Value, json};
use std::io::{self, Read, Write};
pub fn serve() -> io::Result<()> {
    wallet_sandbox::verify_internet_token()?;
    let mut input = io::stdin();
    let mut length = [0; 4];
    input.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if !(10..=2048).contains(&length) {
        return Err(io::Error::other("frame length"));
    }
    let mut request = vec![0; length];
    input.read_exact(&mut request)?;
    let mut extra = [0];
    if input.read(&mut extra)? != 0 || request[0] != 3 {
        return Err(io::Error::other("frame identity"));
    }
    let result = dispatch(request[1], &request[10..]);
    let (status, result) = match result {
        Ok(value) => (0u16, value),
        Err(error) => (1u16, json!({"error":error.to_string()})),
    };
    let bytes = serde_json::to_vec(&result).map_err(io::Error::other)?;
    if bytes.len() > 1800 {
        return Err(io::Error::other("response length"));
    }
    let mut reply = request[..10].to_vec();
    reply.extend(status.to_le_bytes());
    reply.extend(bytes);
    let mut output = io::stdout();
    output.write_all(&(reply.len() as u32).to_le_bytes())?;
    output.write_all(&reply)?;
    output.flush()
}
fn dispatch(opcode: u8, mut data: &[u8]) -> io::Result<Value> {
    let mut fields = Vec::new();
    while !data.is_empty() {
        if data.len() < 2 || fields.len() >= 3 {
            return Err(io::Error::other("invalid fields"));
        }
        let length = u16::from_le_bytes(data[..2].try_into().unwrap()) as usize;
        if length > 1024 || data.len() < length + 2 {
            return Err(io::Error::other("field length"));
        }
        fields.push(
            std::str::from_utf8(&data[2..length + 2])
                .map_err(|_| io::Error::other("field UTF8"))?,
        );
        data = &data[length + 2..];
    }
    match (opcode, fields.as_slice()) {
        (18, [account]) => wallet_services::tron::balances(account),
        (19, [sender, recipient, amount]) => wallet_services::tron::prepare(sender, recipient, amount),
        (20, [raw, txid, summary]) => wallet_services::tron::broadcast(raw, txid, summary),
        (21, [txid, summary]) => wallet_services::tron::status(txid, summary),
        (14, [account]) => wallet_services::solana::balances(account),
        (15, [sender, recipient, asset_amount]) => wallet_services::solana::prepare(sender, recipient, asset_amount),
        (16, [raw, signature, summary]) => wallet_services::solana::broadcast(raw, signature, summary),
        (17, [signature, summary]) => wallet_services::solana::status(signature, summary),
        (13, [account]) => wallet_services::history_cache::forget(
            &wallet_sandbox::current_container_directory()?, account),
        (9, [sender, recipient]) => wallet_services::prepare_all_bnb(sender, recipient),
        (1, [account]) => wallet_services::bnb_balance(account),
        (2, [sender, recipient, amount]) => wallet_services::prepare_bnb(sender, recipient, amount),
        (3, [raw, hash]) => wallet_services::broadcast_bnb(raw, hash),
        (4, [hash]) => wallet_services::bnb_status(hash),
        (5, [account]) => wallet_services::usdt::balances(account),
        (7 | 8, [account, cursor]) => wallet_services::history_cache::query(
            &wallet_sandbox::current_container_directory()?,
            account,
            cursor,
            opcode == 8,
        ),
        (6, [sender, recipient, amount]) => {
            wallet_services::usdt::prepare(sender, recipient, amount)
        }
        _ => Err(io::Error::other("unsupported public request")),
    }
}
