mod args;
mod client;
mod commands;
mod ctx;

use args::Command;
use ctx::Ctx;

pub use args::Cli;

pub fn run(cli: Cli) -> anyhow::Result<()> {
    let ctx = Ctx::load()?;

    match cli.cmd {
        Command::Provider(cmd) => commands::provider::run(&ctx, cmd),
        Command::Point(args) => commands::point::run(&ctx, args),
        Command::Status(args) => commands::status::run(&ctx, args),
        Command::Usage(args) => commands::usage::run(&ctx, args),
        Command::Pin(args) => commands::pin::run(&ctx, args),
        Command::Settings(args) => commands::settings::run(&ctx, args),
        Command::Completions(cmd) => commands::completions::run(cmd),
        Command::Migrate(cmd) => commands::migrate::run(&ctx, cmd),
        Command::Service(cmd) => commands::service::run(&ctx, cmd),
        Command::Serve => commands::service::serve(&ctx),
    }
}
