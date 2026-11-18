mod vault;

fn main() -> std::process::ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--vault-session") {
        return match vault::serve() {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(_) => std::process::ExitCode::from(2),
        };
    }
    match wallet_ipc::serve(
        wallet_ipc::Role::Signer,
        std::io::stdin(),
        std::io::stdout(),
    ) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(_) => std::process::ExitCode::from(2),
    }
}
