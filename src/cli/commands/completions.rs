use std::io::Write;

use anyhow::Result;
use clap::CommandFactory;
use clap_complete::Shell as Cs;

use crate::cli::args::{Cli, CompletionsCmd};

pub fn run(cmd: CompletionsCmd) -> Result<()> {
    let shell = match cmd {
        CompletionsCmd::Bash => Cs::Bash,
        CompletionsCmd::Zsh => Cs::Zsh,
        CompletionsCmd::Fish => Cs::Fish,
        CompletionsCmd::PowerShell => Cs::PowerShell,
    };

    let mut out = Vec::new();
    clap_complete::generate(shell, &mut Cli::command(), "awitch", &mut out);
    std::io::stdout().lock().write_all(&out)?;
    Ok(())
}
