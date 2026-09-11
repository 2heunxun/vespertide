//! N-API bridge: the `vespertide` CLI as a Node addon.
//!
//! One exported function, `main(args)`, which is [`vespertide_cli::run_cli`]
//! fed the arguments Node received. Output, prompts and exit codes are the
//! binary's own; `main.js` only hands the code to the process.

use napi_derive::napi;

/// Run the CLI with `args` (everything after the program name) and return
/// the exit code the binary would have exited with.
#[napi]
#[cfg(not(tarpaulin_include))]
pub async fn main(args: Vec<String>) -> i32 {
    // clap reads the usage line's binary name from argv[0].
    let argv = std::iter::once("vespertide".to_string()).chain(args);
    i32::from(vespertide_cli::run_cli(argv).await)
}
