//! Library face of the `vespertide` CLI.
//!
//! The binary in `main.rs` is a shell around [`run_cli`]; the N-API bridge
//! calls the same function with the arguments it received from Node. Both go
//! through one parse and one dispatch, so a flag added to [`Commands`]
//! reaches every host the same way.

use std::ffi::OsString;
use std::fmt;
use std::io::Write;

use clap::{CommandFactory, Parser};

mod cli;
mod commands;
mod parallel_config;
#[cfg(test)]
mod test_support;
mod utils;

pub use cli::{BackendArg, Cli, Commands};
pub use commands::erd::ErdFormat;
use commands::{
    cmd_diff, cmd_erd_with_filters, cmd_export, cmd_init, cmd_log, cmd_new, cmd_revision, cmd_sql,
    cmd_status,
};

/// Why a CLI invocation did not succeed.
///
/// The two arms carry different exit codes: clap decides the code for a
/// usage error (`2`, or `0` for `--help` / `--version`, which are "errors"
/// only in clap's model), while a command that ran and failed is always `1`
/// — the same code `main() -> Result` used to produce.
#[derive(Debug)]
#[non_exhaustive]
pub enum CliError {
    /// The arguments did not parse. clap has already printed its message.
    Usage(clap::Error),
    /// The command ran and returned an error.
    Failed(anyhow::Error),
}

impl CliError {
    /// The process exit code this error stands for.
    pub fn exit_code(&self) -> u8 {
        match self {
            // clap reports 0 or 2; anything else is treated as a failure.
            Self::Usage(err) => u8::try_from(err.exit_code()).unwrap_or(1),
            Self::Failed(_) => 1,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(err) => err.fmt(f),
            // The alternate form carries the whole context chain, not just the
            // outermost message — what a host that only sees `to_string()` needs.
            Self::Failed(err) => write!(f, "{err:#}"),
        }
    }
}

impl std::error::Error for CliError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Usage(err) => Some(err),
            Self::Failed(err) => Some(err.as_ref()),
        }
    }
}

/// Dispatch a parsed command line.
///
/// Pure routing — every arm is one `cmd_*` call — which is why it carries no
/// coverage of its own: the integration tests exercise it through the built
/// binary, exactly as they did when this `match` lived in `main`.
#[cfg(not(tarpaulin_include))]
pub async fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Some(Commands::Diff) => cmd_diff().await,
        Some(Commands::Sql {
            backend,
            transaction,
        }) => cmd_sql(backend.into(), transaction).await,
        Some(Commands::Log {
            backend,
            transaction,
        }) => cmd_log(backend.into(), transaction).await,
        Some(Commands::New { name, format }) => cmd_new(name, format).await,
        Some(Commands::Status) => cmd_status().await,
        Some(Commands::Revision {
            message,
            fill_with,
            delete_null_rows,
        }) => cmd_revision(message, fill_with, delete_null_rows).await,
        Some(Commands::Init) => cmd_init().await,
        Some(Commands::Export { orm, export_dir }) => cmd_export(orm, export_dir).await,
        Some(Commands::Erd {
            format,
            output,
            include,
            exclude,
            depth,
        }) => cmd_erd_with_filters(format, output, include, exclude, depth).await,
        None => {
            // No subcommand: show help and exit successfully.
            Cli::command().print_help()?;
            println!();
            Ok(())
        }
    }
}

/// Parse `args` — `argv[0]` first, since clap reads the usage line's binary
/// name from it — and run the command.
///
/// This is the entry point for embedders. It never calls `process::exit`:
/// `--help` and `--version` print and return `Ok`, a usage error prints and
/// returns [`CliError::Usage`], so a host such as Node keeps its process.
/// Arguments are taken as `OsString`-convertible values so a path that is not
/// valid Unicode reaches clap intact, as it does from the shell.
pub async fn main<I, T>(args: I) -> Result<(), CliError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(err) => {
            // Swallow broken-pipe errors, as clap's own exit path does.
            let _ = err.print();
            return if err.exit_code() == 0 {
                Ok(())
            } else {
                Err(CliError::Usage(err))
            };
        }
    };
    run(cli).await.map_err(CliError::Failed)
}

/// Run the command line and return the process exit code.
///
/// A failed command is reported as `Error: {err:?}` on stderr — the text
/// std's `Termination` printed when `main` returned `Result` — and, like
/// that path, a stderr that cannot be written to is ignored rather than a
/// reason to panic.
pub async fn run_cli<I, T>(args: I) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let code = match main(args).await {
        Ok(()) => 0,
        Err(err) => {
            if let CliError::Failed(cause) = &err {
                let _ = writeln!(std::io::stderr(), "Error: {cause:?}");
            }
            err.exit_code()
        }
    };
    // A host that keeps the process alive (the N-API bridge) gets no exit-time
    // flush, so drain line-buffered stdout before handing the code back.
    let _ = std::io::stdout().flush();
    code
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::*;
    use rstest::rstest;
    use serial_test::serial;

    fn argv(rest: &[&str]) -> Vec<String> {
        std::iter::once("vespertide")
            .chain(rest.iter().copied())
            .map(str::to_string)
            .collect()
    }

    /// An argument the platform cannot represent as UTF-8.
    #[cfg(unix)]
    fn non_unicode_arg() -> OsString {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(vec![0xff])
    }

    #[cfg(windows)]
    fn non_unicode_arg() -> OsString {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&[0xD800])
    }

    /// `--help`, `--version` and a bare invocation all print and succeed —
    /// none of them may reach `process::exit`, or an embedding host dies.
    #[rstest]
    #[case::help(&["--help"])]
    #[case::version(&["--version"])]
    #[case::bare(&[])]
    #[tokio::test]
    async fn main_prints_and_succeeds(#[case] rest: &[&str]) {
        assert!(main(argv(rest)).await.is_ok());
    }

    /// A usage error surfaces as `Usage` with clap's exit code and message,
    /// after clap has printed that message itself.
    #[rstest]
    #[case::unknown_subcommand(&["bogus"], "unrecognized subcommand")]
    #[case::bad_value(&["sql", "--backend", "nope"], "invalid value")]
    #[tokio::test]
    async fn main_reports_usage_errors(#[case] rest: &[&str], #[case] message: &str) {
        let err = main(argv(rest)).await.unwrap_err();
        assert!(matches!(err, CliError::Usage(_)));
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains(message));
        assert!(err.source().is_some());
    }

    /// A non-Unicode argument is clap's usage error, as it is from the shell
    /// — not a panic on the way in.
    #[tokio::test]
    async fn main_keeps_non_unicode_arguments_a_usage_error() {
        let args = vec![
            OsString::from("vespertide"),
            OsString::from("new"),
            non_unicode_arg(),
        ];
        let err = main(args).await.unwrap_err();
        assert!(matches!(err, CliError::Usage(_)));
        assert_eq!(err.exit_code(), 2);
    }

    /// A command that ran and failed is `Failed` with its cause at the API
    /// layer and exit code 1 at the process layer.
    #[tokio::test]
    #[serial]
    async fn failed_command_is_exit_one_at_every_layer() {
        let tmp = tempfile::tempdir().unwrap();
        let _guard = test_support::CwdGuard::new(tmp.path());
        let err = main(argv(&["status"])).await.unwrap_err();
        assert!(matches!(err, CliError::Failed(_)));
        assert!(err.to_string().contains("vespertide.json"));
        assert!(err.source().is_some());
        assert_eq!(run_cli(argv(&["status"])).await, 1);
    }

    #[rstest]
    #[case::success(&["--version"], 0)]
    #[case::usage(&["bogus"], 2)]
    #[tokio::test]
    async fn run_cli_maps_exit_codes(#[case] rest: &[&str], #[case] expected: u8) {
        assert_eq!(run_cli(argv(rest)).await, expected);
    }
}
