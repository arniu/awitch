use std::path::Path;

use super::point;
use crate::cli::args::ServiceCmd;
use crate::cli::ctx::Ctx;

pub fn serve(ctx: &Ctx) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(crate::server::Server::serve(&ctx.config_dir))
}

pub fn run(ctx: &Ctx, cmd: ServiceCmd) -> anyhow::Result<()> {
    let action = match cmd {
        ServiceCmd::Uninstall => {
            if point::any_pointed(ctx) {
                eprintln!(
                    "warning: agents are still pointed — run 'awitch point undo --all' to fix"
                );
            }

            supervisor::Action::Uninstall
        }
        ServiceCmd::Install => supervisor::Action::Install,
        ServiceCmd::Start => supervisor::Action::Start,
        ServiceCmd::Stop => supervisor::Action::Stop,
        ServiceCmd::Restart => supervisor::Action::Restart,
        ServiceCmd::Status => supervisor::Action::Status,
        ServiceCmd::Logs => supervisor::Action::Logs,
    };

    let service = make_service(&ctx.config_dir);
    Ok(supervisor::run(&service, action)?)
}

fn make_service(dir: &Path) -> supervisor::Service {
    supervisor::Service {
        name: "awitch".into(),
        label: "com.awitch.gateway".into(),
        program: std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        args: vec!["serve".into()],
        log_dir: dir.join("logs"),
        pid_file: dir.join("server.pid"),
    }
}
