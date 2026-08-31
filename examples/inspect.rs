//! Inspect one file: detection, routing, member tree, chunk stats.
//!
//! ```text
//! cargo run --example inspect -- path/to/file.docx [media/type]
//! ```

#[path = "shared/report.rs"]
#[allow(dead_code)]
mod report;

use std::path::PathBuf;

use async_chunker::MediaType;
use report::{Route, analyze, human};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next().map(PathBuf::from) else {
        eprintln!("usage: inspect <file> [declared-media-type]");
        std::process::exit(2);
    };
    let declared = args.next().map(|text| {
        MediaType::parse(&text).unwrap_or_else(|error| {
            eprintln!("invalid media type {text:?}: {error}");
            std::process::exit(2);
        })
    });
    let analysis = analyze(&path, declared.as_ref()).unwrap_or_else(|error| {
        eprintln!("cannot read {}: {error}", path.display());
        std::process::exit(1);
    });
    println!("{} — {}", path.display(), human(analysis.input_bytes));
    match &analysis.route {
        Route::Decomposed { tree, facts } => {
            println!("route: decomposed");
            for line in tree {
                println!("  {line}");
            }
            for level in facts {
                println!(
                    "  [{}] {} members, {} entries{}",
                    level.kind.name(),
                    level.member_count,
                    level.entry_count,
                    level
                        .office_kind
                        .map(|kind| format!(", office kind: {kind}"))
                        .unwrap_or_default(),
                );
            }
        }
        Route::Chunked { profile } => println!("route: chunked ({})", profile.name()),
        Route::Opaque { reason } => println!("route: OPAQUE — {reason}"),
    }
    let stats = analysis.stats();
    println!(
        "chunks: {} ({} stored bytes; min {}, max {})",
        stats.count,
        human(stats.total),
        human(stats.min),
        human(stats.max),
    );
}
