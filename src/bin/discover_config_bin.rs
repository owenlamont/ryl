#![forbid(unsafe_code)]
#![deny(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]

use std::env;
use std::path::PathBuf;

use ryl::config::{Overrides, discover_config};

fn main() {
    let mut args = env::args().skip(1);
    let dir = args
        .next()
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    let inputs = vec![dir];
    match discover_config(&inputs, &Overrides::default()) {
        Ok(ctx) => {
            let out = ctx
                .source
                .map(|p| format!("{}", p.display()))
                .unwrap_or_default();
            println!("{out}");
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    }
}
