#![allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
use granit_parser::{Event, Parser, Span, SpannedEventReceiver};
use std::{env, fs::File, io::prelude::*};

/// A sink which discards any event sent.
struct NullSink {}

impl<'a> SpannedEventReceiver<'a> for NullSink {
    fn on_event(&mut self, _: Event<'a>, _: Span) {}
}

/// Parse the given input, returning elapsed time in nanoseconds.
fn do_parse(input: &str) -> Result<u64, String> {
    let mut sink = NullSink {};
    let mut parser = Parser::new_from_str(input);
    let begin = std::time::Instant::now();
    parser
        .load(&mut sink, true)
        .map_err(|e| format!("failed to parse YAML input: {e:?}"))?;
    let end = std::time::Instant::now();
    Ok((end - begin).as_nanos() as u64)
}

fn usage(program: &str) {
    eprintln!("Usage: {program} <input-file> <iterations> [--output-yaml]");
}

fn run() -> Result<(), String> {
    let args: Vec<_> = env::args().collect();
    if args.len() < 3 || args.len() > 4 {
        usage(&args[0]);
        return Err("invalid arguments".to_owned());
    }

    let iterations: u64 = args[2]
        .parse()
        .map_err(|e| format!("invalid iterations '{}': {e}", args[2]))?;
    if iterations == 0 {
        return Err("iterations must be greater than zero".to_owned());
    }

    let output_yaml = args.len() == 4 && args[3] == "--output-yaml";
    if args.len() == 4 && !output_yaml {
        usage(&args[0]);
        return Err(format!(
            "unknown option '{}'; expected --output-yaml",
            args[3]
        ));
    }

    let mut f = File::open(&args[1]).map_err(|e| format!("failed to open '{}': {e}", args[1]))?;
    let mut s = String::new();
    f.read_to_string(&mut s)
        .map_err(|e| format!("failed to read '{}': {e}", args[1]))?;

    // Warmup
    do_parse(&s)?;
    do_parse(&s)?;
    do_parse(&s)?;

    // Bench
    let mut times = Vec::with_capacity(iterations as usize);
    for _ in 0..iterations {
        times.push(do_parse(&s)?);
    }

    let mut sorted_times = times.clone();
    sorted_times.sort_unstable();

    // Compute relevant metrics.
    let sum: u64 = times.iter().sum();
    let avg = sum / iterations;
    let min = sorted_times[0];
    let max = sorted_times[(iterations - 1) as usize];
    let percentile95 = sorted_times[((95 * iterations) / 100) as usize];

    if output_yaml {
        println!("parser: granit-parser");
        println!("input: {}", args[1]);
        println!("average: {avg}");
        println!("min: {min}");
        println!("max: {max}");
        println!("percentile95: {percentile95}");
        println!("iterations: {iterations}");
        println!("times:");
        for time in &times {
            println!("  - {time}");
        }
    } else {
        println!("Average: {}s", (avg as f64) / 1_000_000_000.0);
        println!("Min: {}s", (min as f64) / 1_000_000_000.0);
        println!("Max: {}s", (max as f64) / 1_000_000_000.0);
        println!("95%: {}s", (percentile95 as f64) / 1_000_000_000.0);
    }

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
