use clap::Parser;

/// Simple program to simulate distributed nodes
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    #[arg(short, long, default_value = "foo")]
    pub id: String,

    #[arg(long)]
    pub connect: Option<String>,

    #[arg(long, default_value = "[::1]")]
    pub hostname: String,

    #[arg(short, long, default_value = "0")]
    pub port: u16,
}

