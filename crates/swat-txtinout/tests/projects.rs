//! The test scenarios (private data, not in this repository: read from the folder `SWAT_TESTDATA`
//! names, holding `Osu_1hru`, `Ames_sub1`, `Batanghari` and `Pessel`; skipped without it): every file comes back byte for byte, and every
//! line of every file has a place in its view.

use rustgis_swat_model::TextFile;
use rustgis_swat_txtinout::view::{Kind, Layout, View, columns};
use rustgis_swat_txtinout::{cio, read, write};
use std::path::{Path, PathBuf};

/// What the nested layouts must find.
fn check(name: &str, fname: &str, f: &TextFile, v: &View) {
    let first = |l: usize| f.token(l, 0).unwrap_or("").to_ascii_lowercase();
    let num = |l: usize| f.token(l, 0).is_some_and(|t| t.parse::<f64>().is_ok());
    for r in v.records.iter().filter(|r| r.kind == Kind::Record) {
        let at = format!("{name}/{fname}:{}", r.line + 1);
        match v.layout {
            Layout::Soils => assert!(!r.children.is_empty() && r.children.iter().all(|c| num(c.line)), "{at}"),
            Layout::PlantIni => assert!(r.children.iter().all(|c| !num(c.line)), "{at}"),
            Layout::Wgn => assert!(r.children.len() == 13 && r.children[1..].iter().all(|c| num(c.line)), "{at}"),
            Layout::DecisionTables => {
                let heads: Vec<String> = r.children.iter().filter(|c| c.kind == Kind::Header).map(|c| first(c.line)).collect();
                assert_eq!(heads, ["var", "act_typ"], "{at}");
            }
            Layout::Schedules => {
                let ops = f.token(r.line, 1).and_then(|t| t.parse::<usize>().ok()).unwrap_or(0);
                let autos = r.children.len() - ops;
                assert!(r.children[autos..].iter().all(|c| f.values(c.line).len() >= 4), "{at}");
            }
            _ => {}
        }
    }
}

fn root() -> PathBuf {
    std::env::var_os("SWAT_TESTDATA").map(PathBuf::from).unwrap_or_default()
}

fn files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, out);
            } else if let (Ok(b), Ok(rel)) = (std::fs::read(&p), p.strip_prefix(base)) {
                out.push((rel.to_string_lossy().replace('\\', "/"), b));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

#[test]
fn scenarios() {
    let mut seen = 0;
    for name in ["Osu_1hru", "Ames_sub1", "Batanghari", "Pessel"] {
        let dir = root().join(name);
        if !dir.join("file.cio").exists() {
            continue;
        }
        seen += 1;
        let orig = files(&dir);
        let p = read(orig.clone());
        let back = write(&p);
        let bad: Vec<&str> = orig.iter().zip(&back).filter(|(a, b)| a != b).map(|(a, _)| a.0.as_str()).collect();
        assert!(bad.is_empty() && back.len() == orig.len(), "{name}: not byte-identical {bad:?}");
        let missing = cio::missing(&p);
        eprintln!("{name}: {} files, {} inputs, missing {:?}", orig.len(), cio::inputs(&p).len(), missing);
        let mut others = Vec::new();
        for (fname, f) in &p.files {
            let v = View::of(fname, f);
            let flat = v.flat();
            let lines: Vec<usize> = flat.iter().map(|r| r.line).collect();
            assert_eq!(lines, (0..f.len()).collect::<Vec<_>>(), "{name}/{fname}: lines");
            check(name, fname, f, &v);
            for r in &flat {
                if r.kind == Kind::Other {
                    others.push(format!("{fname}:{}", r.line + 1));
                }
                if r.kind == Kind::Record && v.layout != Layout::Table {
                    let _ = columns(f, r);
                }
            }
        }
        assert!(others.is_empty(), "{name}: lines out of place {others:?}");
    }
    eprintln!("{seen} scenarios");
}
