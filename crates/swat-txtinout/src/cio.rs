//! `file.cio`: the scenario's index. Its first line is a title; each other line is a section
//! (`simulation`, `climate`, `connect`…) and the files of that section, `null` where there is
//! none. Weather files are named again in the climate section's lists (`pcp.cli`…), under the
//! folder of the `*_path` sections, and recall files in `recall.rec`.

use rustgis_swat_model::{Project, TextFile};

/// A section of `file.cio`: its name, its line, and its files by position (`null` kept).
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub name: String,
    pub line: usize,
    pub files: Vec<String>,
}

/// `file.cio` read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cio {
    pub sections: Vec<Section>,
}

impl Cio {
    pub fn parse(f: &TextFile) -> Cio {
        let mut sections = Vec::new();
        for i in 1..f.len() {
            let v = f.values(i);
            let Some((name, files)) = v.split_first() else { continue };
            sections.push(Section { name: name.to_string(), line: i, files: files.iter().map(|s| s.to_string()).collect() });
        }
        Cio { sections }
    }

    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }

    /// The files it names, without `null`, in order.
    pub fn files(&self) -> Vec<String> {
        self.sections.iter().filter(|s| !s.name.ends_with("_path")).flat_map(|s| s.files.iter()).filter(|f| !is_null(f)).cloned().collect()
    }

    /// The folder a kind of weather file is in (`pcp` → `pcp_path`), `""` for the scenario's.
    pub fn path(&self, kind: &str) -> String {
        match self.section(&format!("{kind}_path")).and_then(|s| s.files.first()) {
            Some(p) if !is_null(p) => {
                let p = p.replace('\\', "/");
                if p.ends_with('/') { p } else { format!("{p}/") }
            }
            _ => String::new(),
        }
    }
}

fn is_null(s: &str) -> bool {
    s.eq_ignore_ascii_case("null")
}

/// The climate lists and the kind of weather file each names.
const LISTS: [(&str, &str); 6] =
    [("pcp.cli", "pcp"), ("tmp.cli", "tmp"), ("slr.cli", "slr"), ("hmd.cli", "hmd"), ("wnd.cli", "wnd"), ("pet.cli", "pet")];

/// Every input file the scenario names: `file.cio`'s, the weather files of its climate lists,
/// and `recall.rec`'s files. `file.cio` itself first.
pub fn inputs(p: &Project) -> Vec<String> {
    let Some(cio_file) = p.file("file.cio") else { return Vec::new() };
    let cio = Cio::parse(cio_file);
    let mut out = vec!["file.cio".to_string()];
    for f in cio.files() {
        if !out.contains(&f) {
            out.push(f);
        }
    }
    for (list, kind) in LISTS {
        // The list as file.cio names it, or by its usual name.
        let Some(lf) = p.file(list) else { continue };
        if !out.iter().any(|o| o == list) {
            continue;
        }
        let dir = cio.path(kind);
        for i in 2..lf.len() {
            if let Some(name) = lf.token(i, 0) {
                let n = format!("{dir}{name}");
                if !out.contains(&n) {
                    out.push(n);
                }
            }
        }
    }
    if let Some(rec) = cio.section("recall").and_then(|s| s.files.first()).filter(|f| !is_null(f)).and_then(|n| p.file(n)) {
        // id, name, type, file: the file is the fourth value.
        for i in 2..rec.len() {
            if let Some(name) = rec.token(i, 3) {
                let n = name.to_string();
                if !out.contains(&n) {
                    out.push(n);
                }
            }
        }
    }
    out
}

/// The input files the scenario names but doesn't have.
pub fn missing(p: &Project) -> Vec<String> {
    inputs(p).into_iter().filter(|f| p.file(f).is_none()).collect()
}
