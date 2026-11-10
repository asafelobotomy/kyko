use clap::Parser;

/// kykod: the kyko session holder, which keeps terminal sessions running.
#[derive(Parser)]
#[command(version, about)]
struct Cli {}

fn main() {
    Cli::parse();
    eprintln!("kykod: nothing to do yet (the session holder arrives in 0.0.2)");
    std::process::exit(1);
}
