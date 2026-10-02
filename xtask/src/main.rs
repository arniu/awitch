mod corpus;
mod extract;
mod fetch;
mod manifest;

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let outcome = match (args.next().as_deref(), args.next().as_deref()) {
        (Some("fixtures"), Some("sync")) => corpus::sync(args.next().as_deref()),
        (Some("fixtures"), Some("check")) => corpus::check(),
        (Some("fixtures"), Some("update")) => corpus::update(args.next().as_deref()),
        _ => {
            eprintln!("usage: cargo xtask fixtures <sync [fixture] | check | update [repo]>");
            return ExitCode::from(2);
        }
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}
