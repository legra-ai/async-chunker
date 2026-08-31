//! Measure chunk-level dedup between two files — "how much of B is
//! already stored if A was uploaded first?"
//!
//! ```text
//! cargo run --example dedup -- old.docx new.docx
//! ```

#[path = "shared/report.rs"]
#[allow(dead_code)]
mod report;

use std::path::PathBuf;

use report::{analyze, human};

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(first), Some(second)) = (
        args.next().map(PathBuf::from),
        args.next().map(PathBuf::from),
    ) else {
        eprintln!("usage: dedup <file-a> <file-b>");
        std::process::exit(2);
    };
    let a = analyze(&first, None).unwrap_or_else(|error| {
        eprintln!("cannot read {}: {error}", first.display());
        std::process::exit(1);
    });
    let b = analyze(&second, None).unwrap_or_else(|error| {
        eprintln!("cannot read {}: {error}", second.display());
        std::process::exit(1);
    });
    let (shared_bytes, shared_chunks) = a.shared_bytes_with(&b);
    let b_stats = b.stats();
    println!(
        "{}: {} in {} chunks",
        first.display(),
        human(a.stats().total),
        a.stats().count
    );
    println!(
        "{}: {} in {} chunks",
        second.display(),
        human(b_stats.total),
        b_stats.count
    );
    let ratio = if b_stats.total == 0 {
        0.0
    } else {
        100.0 * shared_bytes as f64 / b_stats.total as f64
    };
    println!(
        "shared: {shared_chunks} chunks, {} — {ratio:.1}% of {} already stored",
        human(shared_bytes),
        second.display(),
    );
}
