//! `vsg-rs`: VSG's command line (see `vsg_cli`).

use std::process::ExitCode;

mod design;
mod lint;
mod local_rules;
mod testbench;
mod vsg_cli;
mod waivers;

fn main() -> ExitCode {
    vsg_cli::main(&std::env::args().collect::<Vec<_>>())
}
