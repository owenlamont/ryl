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
        Ok(ctx) => match ctx.source {
            Some(p) => println!("{}", p.display()),
            None => println!(),
        },
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    }
}
