use clap::Parser;

use awitch::cli::{self, Cli};
use awitch::config_dir;

fn main() {
    let cli = Cli::parse();
    let run_serve = cli.is_serve();
    let _guard = init_tracing(run_serve);

    if let Err(e) = cli::run(cli) {
        tracing::error!("error: {e:#}");
        std::process::exit(1);
    }
}

fn init_tracing(serve: bool) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let env_filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());

    if serve {
        let logs_dir = config_dir().join("logs");
        if let Err(e) = std::fs::create_dir_all(&logs_dir) {
            eprintln!("warning: cannot create log dir {}: {e}", logs_dir.display());
        }

        let appender = tracing_appender::rolling::Builder::new()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .max_log_files(5)
            .filename_prefix("awitch")
            .filename_suffix("log")
            .build(logs_dir)
            .expect("log file appender");
        let (writer, guard) = tracing_appender::non_blocking(appender);
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_writer(writer)
            .init();
        Some(guard)
    } else {
        tracing_subscriber::fmt().with_env_filter(env_filter).init();
        None
    }
}
