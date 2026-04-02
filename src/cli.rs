use clap::Parser;
use hyper::Uri;

/// Simple program to simulate distributed nodes
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    #[arg(short, long, default_value = "foo")]
    pub id: String,

    #[arg(long)]
    pub connect: Option<Uri>,

    #[arg(long, default_value = "[::]")]
    pub hostname: String,

    #[arg(short, long, default_value = "0")]
    pub port: u16,

    /// Per-second probability that the node will simulate a crash (0.0 = disabled, 1.0 = every second)
    #[arg(long, default_value = "0.0")]
    pub crash_probability: f64,

    /// Seconds to stay "dead" before recovering
    #[arg(long, default_value = "10")]
    pub recovery_time: u64,

    /// Seed for the deterministic RNG (omit for random seed)
    #[arg(long)]
    pub rng_seed: Option<u64>,
}
