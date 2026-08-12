use clap::Subcommand;

#[derive(Debug, Subcommand)]
pub(crate) enum UpdateAction {
    /// Check whether a newer version is available.
    Check {
        #[arg(long)]
        pub(crate) binary: Option<String>,
        #[arg(long)]
        pub(crate) current_version: Option<String>,
    },
    /// Download the latest CLI version and stage it for the next launch.
    Apply,
}
