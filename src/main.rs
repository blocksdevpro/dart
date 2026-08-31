use std::env;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    match dart::cli::run(env::args_os().skip(1)).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("dart: {error}");
            ExitCode::FAILURE
        }
    }
}
