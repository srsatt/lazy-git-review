use std::process::ExitCode;

use clap::Parser;
use lazy_git_review::cli::{Cli, run};
use lazy_git_review::model::Envelope;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let debug = cli.debug;
    match run(cli).await {
        Ok(value) => {
            let response =
                serde_json::to_string(&Envelope::success(value)).expect("serialize response");
            if let Err(error) =
                lazy_git_review::agent_budget::charge_response(response.len() as u64)
            {
                if debug {
                    eprintln!(
                        "lgr v{} [{}]: {:?}",
                        env!("CARGO_PKG_VERSION"),
                        error.code(),
                        error
                    );
                }
                println!(
                    "{}",
                    serde_json::to_string(&Envelope::<()>::failure(&error))
                        .expect("serialize error")
                );
                return ExitCode::from(error.exit_code());
            }
            println!("{response}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            if debug {
                eprintln!(
                    "lgr v{} [{}]: {:?}",
                    env!("CARGO_PKG_VERSION"),
                    error.code(),
                    error
                );
            }
            println!(
                "{}",
                serde_json::to_string(&Envelope::<()>::failure(&error)).expect("serialize error")
            );
            ExitCode::from(error.exit_code())
        }
    }
}
