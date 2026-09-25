//! HTTP API over `fifolio-core`; the only process that opens the database [ARC-003].

use std::process::ExitCode;

use clap::Parser;
use fifolio_server::{Args, Command, openapi, serve};

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    match args.command {
        // Printed before anything is opened or bound, so the spec can be read without a
        // database and while another instance holds the port [SRV-005].
        Some(Command::Openapi) => match openapi().to_pretty_json() {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("fifolio-server: cannot serialize the OpenAPI spec: {error}");
                ExitCode::FAILURE
            }
        },
        None => {
            // Logs go to stderr so stdout stays free for the one thing printed there, the spec.
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .init();
            match serve(args.port, args.database).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("fifolio-server: {error}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}
