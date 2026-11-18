mod public;
fn main() -> std::process::ExitCode {
    if let Some(argument) = std::env::args().nth(2) {
        let result = argument
            .strip_prefix("--proxy=")
            .ok_or_else(|| std::io::Error::other("invalid network argument"))
            .and_then(wallet_sandbox::network_config::validate_proxy)
            .and_then(wallet_services::configure_proxy);
        if result.is_err() || std::env::args().nth(3).is_some() {
            return std::process::ExitCode::from(2);
        }
    }
    if std::env::args().nth(1).as_deref() == Some("--public-request") {
        return match public::serve() {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Public service stopped: {error}");
                std::process::ExitCode::from(2)
            }
        };
    }
    if std::env::args().nth(1).as_deref() == Some("--rpc-probe") {
        return match wallet_services::public_probe() {
            Ok(report) => {
                print!("{report}");
                std::process::ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("Public RPC probe: {error}");
                std::process::ExitCode::from(2)
            }
        };
    }
    match wallet_ipc::serve(
        wallet_ipc::Role::Network,
        std::io::stdin(),
        std::io::stdout(),
    ) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(_) => std::process::ExitCode::from(2),
    }
}
