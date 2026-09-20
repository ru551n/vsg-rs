//! `vsg-rs`: VSG's command line (see `vsg_cli`).

use std::process::ExitCode;

mod local_rules;
mod lsp;
mod mcp;
mod vsg_cli;
mod waivers;

fn main() -> ExitCode {
    vsg_cli::main(&std::env::args().collect::<Vec<_>>())
}
