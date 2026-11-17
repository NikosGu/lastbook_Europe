//! Lastbook Europe's build tools, one command with a subcommand for each job.
//!
//!   cargo run -p lastbook-tools -- <subcommand> [options]
//!
//! Each subcommand lives in its own module and prints its own usage with --help.

use lastbook_tools::library;

const USAGE: &str = "lastbook-tools <subcommand> [options]

  build-library   a LastbookData folder from documents and their sources list
  build-pack      a LastbookData folder as Lastbook-<id>.zip, with its manifest entry

Run a subcommand with --help for its options.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest = args.get(1..).unwrap_or_default();
    let code = match args.first().map(String::as_str) {
        Some("build-library") => library::build::main(rest),
        Some("build-pack") => library::pack::main(rest),
        _ => {
            eprintln!("{USAGE}");
            2
        }
    };
    std::process::exit(code);
}
