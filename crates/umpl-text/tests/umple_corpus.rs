//! Imports every `.ump` file in a folder and reports what was skipped.
//!
//! Not run by default: it needs a corpus, such as the examples in Umple's
//! own repository (`umpleonline/umplibrary`):
//!
//! ```text
//! UMPLE_CORPUS=path/to/umplibrary cargo test -p umpl-text --test umple_corpus -- --ignored --nocapture
//! ```
//!
//! `UMPLE_CORPUS_SHOW=<text>` also lists every problem containing `<text>`;
//! `UMPLE_CORPUS_MEMBERS=1` lists every member read, to look for odd ones.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use umpl_text::{parse, print, umple};

fn ump_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
    entries.sort_by_key(|e| e.path());
    for e in entries {
        let p = e.path();
        if p.is_dir() {
            ump_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "ump") {
            out.push(p);
        }
    }
}

/// A problem message with names and quoted text replaced by `…`, so that
/// problems of one kind count together.
fn category(message: &str) -> String {
    let mut out = String::new();
    let mut quoted = false;
    for c in message.chars() {
        if c == '`' {
            quoted = !quoted;
            if quoted {
                out.push_str("`…`");
            }
        } else if !quoted {
            out.push(c);
        }
    }
    out
}

#[test]
#[ignore = "needs UMPLE_CORPUS"]
fn imports_the_corpus() {
    let dir = PathBuf::from(std::env::var("UMPLE_CORPUS").expect("set UMPLE_CORPUS"));
    let mut files = Vec::new();
    ump_files(&dir, &mut files);
    assert!(!files.is_empty(), "no .ump files in {}", dir.display());

    let mut categories: BTreeMap<String, usize> = BTreeMap::new();
    let mut examples: BTreeMap<String, String> = BTreeMap::new();
    let (mut elements, mut relations, mut clean, mut commented) = (0, 0, 0, 0);
    let mut empty = Vec::new();
    // UMPLE_CORPUS_SHOW=text lists every problem whose message contains it.
    let show = std::env::var("UMPLE_CORPUS_SHOW").ok();
    let members = std::env::var("UMPLE_CORPUS_MEMBERS").is_ok();
    for f in &files {
        let Ok(src) = std::fs::read_to_string(f) else {
            continue;
        };
        let p = umple::import(&src);
        elements += p.doc.elements.len();
        if members {
            for el in p.doc.elements.values() {
                for m in &el.members {
                    println!("member {}: {}: {}", f.display(), el.name, m.to_line());
                }
            }
        }
        relations += p.doc.relations.len();
        if p.problems.is_empty() {
            clean += 1;
        }
        if p.doc.elements.is_empty() {
            empty.push(f.file_name().unwrap().to_string_lossy().into_owned());
        }
        for pr in &p.problems {
            let c = category(&pr.message);
            if show.as_deref().is_some_and(|s| pr.message.contains(s)) {
                println!("{}:{}: {}", f.display(), pr.line, pr.message);
            }
            *categories.entry(c.clone()).or_default() += 1;
            examples
                .entry(c)
                .or_insert_with(|| format!("{}:{}: {}", f.display(), pr.line, pr.message));
        }
        // The `.umpl` text of every import reads back without problems.
        let back = parse(&print(&p.doc));
        assert_eq!(back.problems, vec![], "{}", f.display());
        // Exported again as Umple, every model reads back cleanly with the
        // same classes; relations Umple cannot hold become comments.
        let ump = umple::export(&p.doc);
        let again = umple::import(&ump);
        assert_eq!(again.problems, vec![], "{}:\n{ump}", f.display());
        assert_eq!(
            again.doc.elements.len(),
            p.doc.elements.len(),
            "{}",
            f.display()
        );
        assert!(again.doc.relations.len() <= p.doc.relations.len());
        commented += p.doc.relations.len() - again.doc.relations.len();
    }

    println!(
        "{} files, {clean} without problems; {elements} elements, {relations} relations",
        files.len()
    );
    println!("exported again: {commented} relations became comments");
    let mut by_count: Vec<_> = categories.into_iter().collect();
    by_count.sort_by(|a, b| b.1.cmp(&a.1));
    for (c, n) in &by_count {
        println!("{n:6}  {c}\n        e.g. {}", examples[c]);
    }
    println!("{} files with no classes: {}", empty.len(), empty.join(" "));
}
