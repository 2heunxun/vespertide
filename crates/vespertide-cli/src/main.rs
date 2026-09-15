use std::process::ExitCode;

#[cfg(not(tarpaulin_include))]
#[tokio::main]
async fn main() -> ExitCode {
    ExitCode::from(vespertide_cli::run_cli(std::env::args_os()).await)
}
