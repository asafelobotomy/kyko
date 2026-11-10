use clap::Parser;

/// kyko: a drop-down terminal for the uQ desktop.
#[derive(Parser)]
#[command(version, about)]
struct Cli {}

fn main() {
    Cli::parse();
    eprintln!("kyko: nothing to do yet (the window arrives in 0.0.3)");
    std::process::exit(1);
}
