use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::ledger::BucketWidth;

/// `provider add` help footer:
const AFTER_ADD_HELP: &str = r#"
One `--protocol` per served protocol — a bare protocol claims its canonical
endpoint; `protocol=path` spells the endpoint.

EXAMPLES:
# no template: anthropic on its own endpoint, openai_chat canonical
awitch provider add --base-url https://api.deepseek.com \
    --key-file ~/key \
    --protocol anthropic=/anthropic \
    --protocol openai_chat
# register from a definition file — fields and endpoints come from the file
awitch provider add --file ./deepseek-proxy.toml --key-file ~/key"#;

/// `provider edit` help footer.
const AFTER_EDIT_HELP: &str = r#"
One `--protocol` per served protocol — a bare protocol claims its canonical
endpoint; `protocol=path` spells the endpoint. The flag replaces the whole
served protocol set — omitting a protocol removes it.

EXAMPLES:
# set the served protocols: anthropic on its own endpoint, openai_chat canonical
awitch provider edit deepseek \
    --protocol anthropic=/anthropic \
    --protocol openai_chat
# remove a family: leave it out of the replacement list
awitch provider edit deepseek \
    --protocol anthropic=/anthropic"#;

#[derive(Parser, Debug)]
#[command(name = "awitch", version, about = "A local AI gateway")]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Command,
}

impl Cli {
    pub fn is_serve(&self) -> bool {
        matches!(&self.cmd, Command::Serve)
    }
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Manage providers.
    #[command(subcommand)]
    Provider(ProviderCmd),

    /// Point apps to the gateway.
    Point(PointArgs),

    /// Show overall status.
    Status(StatusArgs),

    /// Show usage.
    Usage(UsageArgs),

    /// Pin apps' routing providers.
    Pin(PinArgs),

    /// Show or set gateway settings.
    Settings(SettingsArgs),

    /// Generate a completion script for one shell.
    #[command(subcommand)]
    Completions(CompletionsCmd),

    /// Import providers from another tool.
    #[command(subcommand)]
    Migrate(MigrateCmd),

    /// Manage the gateway service.
    #[command(subcommand)]
    Service(ServiceCmd),

    /// Internal server entry
    ///
    /// The supervised unit runs `awitch serve`
    #[command(hide = true)]
    Serve,
}

#[derive(Parser, Debug)]
pub struct PointArgs {
    /// App to point.
    #[arg(long, conflicts_with = "all")]
    pub app: Option<String>,
    /// Apply to every app.
    #[arg(long)]
    pub all: bool,
    /// Skip installing the gateway service (pointing only).
    #[arg(long)]
    pub no_service: bool,
    #[command(subcommand)]
    pub action: Option<PointAction>,
}

#[derive(Subcommand, Debug)]
pub enum PointAction {
    /// Undo pointing: restore the original config.
    Undo {
        /// App to undo.
        #[arg(long, conflicts_with = "all", required_unless_present = "all")]
        app: Option<String>,
        /// Apply to every app.
        #[arg(long)]
        all: bool,
    },
    /// Reset a corrupt undo record.
    Reset {
        /// App to reset.
        #[arg(long, conflicts_with = "all", required_unless_present = "all")]
        app: Option<String>,
        /// Apply to every app.
        #[arg(long)]
        all: bool,
    },
}

#[derive(Args, Debug)]
pub struct StatusArgs {
    /// Focus one app's routing details.
    #[arg(long)]
    pub app: Option<String>,
}

#[derive(Args, Debug)]
pub struct UsageArgs {
    /// Focus one app's usage.
    #[arg(long, conflicts_with = "provider")]
    pub app: Option<String>,
    /// Focus one provider's usage.
    #[arg(long, conflicts_with = "app")]
    pub provider: Option<String>,
    /// Bucket the report per hour or per day.
    ///
    /// Default: per day.
    #[arg(long, value_name = "hour|day", default_value = "day")]
    pub by: BucketWidth,
    /// Report from this time on — a month (YYYY-MM), a date (YYYY-MM-DD) or
    /// an instant (RFC 3339).
    ///
    /// Default: from the start of the current month.
    #[arg(long)]
    pub since: Option<String>,
}

/// `pin` with no subcommand pins a provider: its default action is add.
#[derive(Parser, Debug)]
pub struct PinArgs {
    /// Provider id to pin (candidate-set add).
    pub provider: Option<String>,
    /// App to pin.
    #[arg(long, conflicts_with = "all", requires = "provider")]
    pub app: Option<String>,
    /// Apply to every app.
    #[arg(long, requires = "provider")]
    pub all: bool,
    #[command(subcommand)]
    pub action: Option<PinAction>,
}

/// The pin subcommands beyond the default add.
#[derive(Subcommand, Debug)]
pub enum PinAction {
    /// Remove one provider from an app's pinned candidates.
    Remove {
        /// Provider id to unpin.
        provider: String,
        /// App to unpin.
        #[arg(long, conflicts_with = "all", required_unless_present = "all")]
        app: Option<String>,
        /// Remove from every app.
        #[arg(long)]
        all: bool,
    },
    /// Clear an app's candidates (→ auto routing).
    Clear {
        /// App to clear.
        #[arg(long, conflicts_with = "all", required_unless_present = "all")]
        app: Option<String>,
        /// Clear every app.
        #[arg(long)]
        all: bool,
    },
    /// Show which providers each app is pinned to.
    List {
        /// Only this app.
        #[arg(long)]
        app: Option<String>,
    },
}

#[derive(Args, Debug)]
pub struct SettingsArgs {
    /// Settings field to show or set.
    pub field: Option<String>,
    /// New value; a field without a value shows its current one.
    pub value: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum ProviderCmd {
    /// Register a provider.
    #[command(after_help = AFTER_ADD_HELP)]
    Add {
        /// Built-in provider template id.
        #[arg(long, short = 't', conflicts_with = "file")]
        template: Option<String>,
        /// Provider definition file — its fields define the record.
        ///
        /// A template-free add: the fields and endpoints come from the file.
        #[arg(long, short = 'f')]
        file: Option<PathBuf>,
        /// API base URL.
        ///
        /// Required unless -t or --file is given.
        #[arg(
            long,
            required_unless_present_any = ["template", "file"],
            conflicts_with_all = ["template", "file"]
        )]
        base_url: Option<String>,
        /// API key (prefer --key-file).
        ///
        /// A literal key is visible in the shell history and process list.
        ///
        /// Pass '-' to read the key from stdin.
        #[arg(long, conflicts_with = "key_file")]
        key: Option<String>,
        /// Read the API key from a file.
        ///
        /// Use it instead of the command line.
        ///
        /// A trailing newline is stripped.
        #[arg(long, conflicts_with = "key")]
        key_file: Option<PathBuf>,
        /// Display name.
        #[arg(long, conflicts_with_all = ["template", "file"])]
        name: Option<String>,
        /// A served protocol family; repeatable.
        #[arg(long, conflicts_with_all = ["template", "file"])]
        protocol: Vec<String>,
        /// Model-list endpoint URL.
        #[arg(long, conflicts_with_all = ["template", "file"])]
        models_url: Option<String>,
        /// Balance endpoint URL.
        #[arg(long, conflicts_with_all = ["template", "file"])]
        balance_url: Option<String>,
        /// Skip the post-add endpoint probe.
        #[arg(long)]
        no_verify: bool,
    },
    /// List built-in provider templates.
    #[command(visible_alias = "templates")]
    Template(TemplateArgs),
    /// List providers.
    List,
    /// Show a provider.
    Show {
        /// Provider id.
        id: String,
    },
    /// Edit a provider.
    #[command(after_help = AFTER_EDIT_HELP)]
    Edit {
        /// Provider id.
        id: String,
        /// API base URL.
        #[arg(long)]
        base_url: Option<String>,
        /// New API key (prefer --key-file).
        ///
        /// A literal key is visible in the shell history and process list.
        ///
        /// Pass '-' to read the key from stdin.
        #[arg(long, conflicts_with = "key_file")]
        key: Option<String>,
        /// Read the API key from a file.
        ///
        /// Use it instead of the command line.
        ///
        /// A trailing newline is stripped.
        #[arg(long, conflicts_with = "key")]
        key_file: Option<PathBuf>,
        /// Display name.
        #[arg(long)]
        name: Option<String>,
        /// A served protocol family; repeatable.
        #[arg(long)]
        protocol: Vec<String>,
        /// Model-list endpoint URL.
        #[arg(long)]
        models_url: Option<String>,
        /// Balance endpoint URL.
        #[arg(long)]
        balance_url: Option<String>,
    },
    /// Remove a provider.
    Delete {
        /// Provider id.
        id: String,
    },
    /// Query a provider's balance.
    Balance {
        /// Provider id.
        id: String,
    },
    /// Test a provider's connectivity.
    Probe {
        /// Provider id.
        id: String,
    },
}

#[derive(Args, Debug)]
pub struct TemplateArgs {
    /// Provider template id (omit to list all).
    pub id: Option<String>,
    /// Output file path.
    ///
    /// Default: <id>.toml in the current directory.
    #[arg(long, short = 'o', requires = "id")]
    pub output: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
pub enum CompletionsCmd {
    /// Print bash completions.
    ///
    /// Install:
    ///
    /// mkdir -p ~/.local/share/bash-completion/completions && awitch completions bash > ~/.local/share/bash-completion/completions/awitch
    ///
    /// Uninstall:
    ///
    /// rm ~/.local/share/bash-completion/completions/awitch
    Bash,
    /// Print zsh completions.
    ///
    /// Install:
    ///
    /// mkdir -p ~/.zfunc && awitch completions zsh > ~/.zfunc/_awitch && echo 'fpath=(~/.zfunc $fpath); autoload -Uz compinit; compinit' >> ~/.zshrc
    ///
    /// Uninstall:
    ///
    /// rm ~/.zfunc/_awitch && grep -v 'fpath=(~/.zfunc' ~/.zshrc > ~/.zshrc.tmp && mv ~/.zshrc.tmp ~/.zshrc
    ///
    /// Start a new shell to load the completions.
    Zsh,
    /// Print fish completions.
    ///
    /// Install:
    ///
    /// mkdir -p ~/.config/fish/completions && awitch completions fish > ~/.config/fish/completions/awitch.fish
    ///
    /// Uninstall:
    ///
    /// rm ~/.config/fish/completions/awitch.fish
    Fish,
    /// Print PowerShell completions.
    ///
    /// Install:
    ///
    /// New-Item -ItemType Directory -Force (Split-Path $PROFILE) | Out-Null; Add-Content $PROFILE 'awitch completions powershell | Out-String | Invoke-Expression'
    ///
    /// Uninstall:
    ///
    /// Set-Content $PROFILE ((Get-Content $PROFILE) -notmatch 'awitch completions')
    #[command(name = "powershell")]
    PowerShell,
}

/// Migrate a source tool's provider pool into awitch.
#[derive(Subcommand, Debug)]
pub enum MigrateCmd {
    /// Import the cc-switch provider pool.
    CcSwitch {
        /// cc-switch database path.
        ///
        /// Default: ~/.cc-switch/cc-switch.db.
        #[arg(long)]
        db: Option<PathBuf>,
        /// Derive and print the plan, write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Skip the post-import endpoint probe.
        ///
        /// Each imported provider's endpoints are probed after the import.
        #[arg(long)]
        no_verify: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum ServiceCmd {
    /// Install the gateway service.
    Install,
    /// Remove the gateway service.
    Uninstall,
    /// Start the gateway service.
    Start,
    /// Stop the gateway service.
    Stop,
    /// Restart the gateway service.
    Restart,
    /// Show service status.
    Status,
    /// Tail the service log.
    Logs,
}
