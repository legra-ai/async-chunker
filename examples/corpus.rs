//! Fetch a corpus of well-known real-world files (into `corpus/`,
//! which stays out of git) and sweep every file in it through the
//! chunker — acceptance, routing, and chunk stats per file.
//!
//! ```text
//! cargo run --example corpus              # fetch + sweep ./corpus
//! cargo run --example corpus -- --no-fetch my-dir
//! ```
//!
//! Drop your own files into the directory to include them in the
//! sweep. Add well-known, freely-redistributable samples to
//! `MANIFEST` — pinned, stable URLs only.

#[path = "shared/report.rs"]
#[allow(dead_code)]
mod report;

use std::io::Read;
use std::path::{Path, PathBuf};

use report::{Route, analyze, human};

/// Well-known sample files: `(file name, pinned URL)`.
const MANIFEST: &[(&str, &str)] = &[
    (
        "dummy.pdf",
        "https://www.w3.org/WAI/ER/tests/xhtml/testfiles/resources/pdf/dummy.pdf",
    ),
    (
        "calibre-demo.docx",
        "https://calibre-ebook.com/downloads/demos/demo.docx",
    ),
    (
        "excelize-book1.xlsx",
        "https://raw.githubusercontent.com/qax-os/excelize/master/test/Book1.xlsx",
    ),
    (
        "python-pptx-test.pptx",
        "https://raw.githubusercontent.com/scanny/python-pptx/master/features/steps/test_files/test.pptx",
    ),
    (
        "hello-world.zip",
        "https://codeload.github.com/octocat/Hello-World/zip/7fd1a60b01f91b314f59955a4e4d4e80d8edf11d",
    ),
    (
        "hello-world.tar.gz",
        "https://codeload.github.com/octocat/Hello-World/tar.gz/7fd1a60b01f91b314f59955a4e4d4e80d8edf11d",
    ),
    (
        "big-buck-bunny-360-10s.mp4",
        "https://test-videos.co.uk/vids/bigbuckbunny/mp4/h264/360/Big_Buck_Bunny_360_10s_1MB.mp4",
    ),
    (
        "big-buck-bunny-360-10s.webm",
        "https://test-videos.co.uk/vids/bigbuckbunny/webm/vp8/360/Big_Buck_Bunny_360_10s_1MB.webm",
    ),
    (
        "soundhelix-song-1.mp3",
        "https://www.soundhelix.com/examples/mp3/SoundHelix-Song-1.mp3",
    ),
    (
        "bipbop-gear1-seq0.ts",
        "https://devstreaming-cdn.apple.com/videos/streaming/examples/bipbop_4x3/gear1/fileSequence0.ts",
    ),
];

fn fetch(dir: &Path) {
    std::fs::create_dir_all(dir).expect("create the corpus directory");
    for (name, url) in MANIFEST {
        let target = dir.join(name);
        if target.exists() {
            continue;
        }
        print!("fetching {name} … ");
        match ureq::get(url).call() {
            Ok(response) => {
                let mut bytes = Vec::new();
                if let Err(error) = response.into_reader().read_to_end(&mut bytes) {
                    println!("read failed: {error}");
                    continue;
                }
                std::fs::write(&target, &bytes).expect("write the corpus file");
                println!("{}", human(bytes.len() as u64));
            }
            Err(error) => println!("failed: {error}"),
        }
    }
}

fn main() {
    let mut fetch_files = true;
    let mut dir = PathBuf::from("corpus");
    for arg in std::env::args().skip(1) {
        if arg == "--no-fetch" {
            fetch_files = false;
        } else {
            dir = PathBuf::from(arg);
        }
    }
    if fetch_files {
        fetch(&dir);
    }
    let mut names: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| {
            eprintln!("cannot read {}: {error}", dir.display());
            std::process::exit(1);
        })
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect();
    names.sort();
    println!();
    println!(
        "{:<32} {:>10} {:<28} {:>7} {:>10}",
        "file", "size", "route", "chunks", "stored"
    );
    let mut opaque = 0usize;
    for path in &names {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        match analyze(path, None) {
            Ok(analysis) => {
                let stats = analysis.stats();
                let route = match &analysis.route {
                    Route::Decomposed { facts, .. } => {
                        let members: u64 = facts.iter().map(|level| level.member_count).sum();
                        format!("decomposed ({members} members)")
                    }
                    Route::Chunked { profile } => profile.name().to_owned(),
                    Route::Opaque { reason } => {
                        opaque += 1;
                        format!("OPAQUE: {reason}")
                    }
                };
                println!(
                    "{:<32} {:>10} {:<28} {:>7} {:>10}",
                    name,
                    human(analysis.input_bytes),
                    route,
                    stats.count,
                    human(stats.total),
                );
            }
            Err(error) => println!("{name:<32} unreadable: {error}"),
        }
    }
    println!();
    println!(
        "{} files, {} opaque — run `--example inspect -- corpus/<file>` for detail",
        names.len(),
        opaque
    );
}
