//! What a TxtInOut file's lines are. Most files are a table: a title, a line of column names and
//! a record a line. Some nest: a soil owns its layers' lines, a plant community its plants', a
//! schedule its automatic tables and operations, a decision table its conditions and actions, a
//! weather generator station its twelve months, a calibration change its conditions. A view
//! never fails: lines it can't place are [`Kind::Other`], and an edit is always made to tokens
//! of the file's lines, so the file stays as it was around it.

use rustgis_swat_model::TextFile;

/// How a file is laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// `file.cio`: a section a line.
    Cio,
    /// `print.prt`: pairs of names and values, then the objects' table.
    Print,
    /// A title, column names, a record a line (most files, `*.con` with their outflows too).
    Table,
    /// A title, a count, column names, a record a line (`cal_parms.cal`).
    CountedTable,
    /// `calibration.cal`: as [`Layout::CountedTable`], a change owning its condition lines.
    Calibration,
    /// `soils.sol`: a soil owning its layers.
    Soils,
    /// `plant.ini`: a community owning its plants.
    PlantIni,
    /// `management.sch`: a schedule owning its automatic tables and its operations.
    Schedules,
    /// `*.dtl`: decision tables, each with its conditions and actions.
    DecisionTables,
    /// `weather-wgn.cli`: stations owning a header and twelve months.
    Wgn,
    /// A weather file: a title, the names and values of its station, a day (or step) a line.
    Series,
}

impl Layout {
    /// The layout of a file by its name (folders and case aside).
    pub fn of(name: &str) -> Layout {
        let base = name.rsplit('/').next().unwrap_or(name).to_ascii_lowercase();
        let ext = base.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
        match base.as_str() {
            "file.cio" => Layout::Cio,
            "print.prt" => Layout::Print,
            "cal_parms.cal" => Layout::CountedTable,
            "calibration.cal" => Layout::Calibration,
            "soils.sol" => Layout::Soils,
            "plant.ini" => Layout::PlantIni,
            "management.sch" => Layout::Schedules,
            "weather-wgn.cli" => Layout::Wgn,
            _ => match ext {
                "dtl" => Layout::DecisionTables,
                "pcp" | "tmp" | "tem" | "slr" | "hmd" | "wnd" | "pet" => Layout::Series,
                _ => Layout::Table,
            },
        }
    }
}

/// What a line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Title,
    Count,
    /// Column names.
    Header,
    /// Values, named by a header's columns.
    Record,
    Blank,
    /// A line the layout doesn't expect.
    Other,
}

/// A line, and the lines it owns.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub line: usize,
    pub kind: Kind,
    /// The header naming a record's values: its line, and the column of its first value.
    pub header: Option<(usize, usize)>,
    pub children: Vec<Record>,
}

impl Record {
    fn new(line: usize, kind: Kind) -> Record {
        Record { line, kind, header: None, children: Vec::new() }
    }

    fn rec(line: usize, header: Option<(usize, usize)>) -> Record {
        Record { line, kind: Kind::Record, header, children: Vec::new() }
    }

    /// The record's name: its first value.
    pub fn name<'a>(&self, f: &'a TextFile) -> Option<&'a str> {
        f.token(self.line, 0)
    }
}

/// A file's lines as records.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub layout: Layout,
    pub records: Vec<Record>,
}

impl View {
    pub fn of(name: &str, f: &TextFile) -> View {
        let layout = Layout::of(name);
        let records = match layout {
            Layout::Cio => cio(f),
            Layout::Print => print(f),
            Layout::Table => table(f, 1),
            Layout::CountedTable => counted(f, false),
            Layout::Calibration => counted(f, true),
            Layout::Soils => owners(f, 7, |v| count(v, 1)),
            Layout::PlantIni => owners(f, 3, |v| count(v, 1)),
            Layout::Schedules => schedules(f),
            Layout::DecisionTables => dtl(f),
            Layout::Wgn => wgn(f),
            Layout::Series => series(f),
        };
        View { layout, records }
    }

    /// Every record, children after their owner, in the order of the lines.
    pub fn flat(&self) -> Vec<&Record> {
        fn walk<'a>(r: &'a Record, out: &mut Vec<&'a Record>) {
            out.push(r);
            for c in &r.children {
                walk(c, out);
            }
        }
        let mut out = Vec::new();
        for r in &self.records {
            walk(r, &mut out);
        }
        out
    }
}

/// The names of a record's values, one a value: the header's from its first column; past the
/// header's end, a repeated group (a connection's outflows: `obj_typ obj_id hyd_typ frac`)
/// numbered from 2, else `col<k>`.
pub fn columns(f: &TextFile, r: &Record) -> Vec<String> {
    let n = f.values(r.line).len();
    let Some((h, first)) = r.header else { return (1..=n).map(|k| format!("col{k}")).collect() };
    let names: Vec<&str> = f.values(h).into_iter().skip(first).collect();
    let group = names.iter().position(|c| c.eq_ignore_ascii_case("obj_typ")).filter(|&g| names.len() - g == 4);
    (0..n)
        .map(|k| match (names.get(k), group) {
            (Some(c), _) => c.to_string(),
            (None, Some(g)) => {
                let i = k - g;
                format!("{}_{}", names[g + i % 4], i / 4 + 1)
            }
            (None, None) => format!("col{}", k + 1),
        })
        .collect()
}

fn count(v: &[&str], k: usize) -> usize {
    v.get(k).and_then(|s| s.parse::<usize>().ok()).unwrap_or(0)
}

fn blank(f: &TextFile, i: usize) -> bool {
    f.line(i).is_none_or(|l| l.trim().is_empty())
}

/// Lines `i..` as records of the header at `h` (blank lines as blanks).
fn rows(f: &TextFile, from: usize, to: usize, header: Option<(usize, usize)>, out: &mut Vec<Record>) {
    for i in from..to {
        out.push(if blank(f, i) { Record::new(i, Kind::Blank) } else { Record::rec(i, header) });
    }
}

fn table(f: &TextFile, header: usize) -> Vec<Record> {
    let mut out = Vec::new();
    for i in 0..header.min(f.len()) {
        out.push(Record::new(i, if i == 0 { Kind::Title } else { Kind::Count }));
    }
    if header < f.len() {
        out.push(Record::new(header, Kind::Header));
    }
    rows(f, header + 1, f.len(), Some((header, 0)), &mut out);
    out
}

fn cio(f: &TextFile) -> Vec<Record> {
    let mut out = Vec::new();
    if !f.is_empty() {
        out.push(Record::new(0, Kind::Title));
    }
    rows(f, 1, f.len(), None, &mut out);
    out
}

fn print(f: &TextFile) -> Vec<Record> {
    let mut out = Vec::new();
    if !f.is_empty() {
        out.push(Record::new(0, Kind::Title));
    }
    let mut i = 1;
    while i < f.len() {
        if blank(f, i) {
            out.push(Record::new(i, Kind::Blank));
            i += 1;
        } else if f.token(i, 0) == Some("objects") {
            out.push(Record::new(i, Kind::Header));
            rows(f, i + 1, f.len(), Some((i, 0)), &mut out);
            break;
        } else {
            out.push(Record::new(i, Kind::Header));
            if i + 1 < f.len() {
                out.push(Record::rec(i + 1, Some((i, 0))));
            }
            i += 2;
        }
    }
    out
}

fn counted(f: &TextFile, conditions: bool) -> Vec<Record> {
    let mut out = Vec::new();
    for (i, k) in [(0, Kind::Title), (1, Kind::Count), (2, Kind::Header)] {
        if i < f.len() {
            out.push(Record::new(i, k));
        }
    }
    let h = Some((2, 0));
    let mut i = 3;
    while i < f.len() {
        if blank(f, i) {
            out.push(Record::new(i, Kind::Blank));
            i += 1;
            continue;
        }
        let mut r = Record::rec(i, h);
        i += 1;
        if conditions {
            // A change's fourth value is its number of conditions: `var op value text` lines.
            let n = count(&f.values(r.line), 3);
            for _ in 0..n {
                if i >= f.len() {
                    break;
                }
                r.children.push(Record::rec(i, None));
                i += 1;
            }
        }
        out.push(r);
    }
    out
}

/// A title, a header, and records owning the next `owned(values)` lines, named by the header
/// from column `split`.
fn owners(f: &TextFile, split: usize, owned: impl Fn(&[&str]) -> usize) -> Vec<Record> {
    let mut out = Vec::new();
    for (i, k) in [(0, Kind::Title), (1, Kind::Header)] {
        if i < f.len() {
            out.push(Record::new(i, k));
        }
    }
    let mut i = 2;
    while i < f.len() {
        if blank(f, i) {
            out.push(Record::new(i, Kind::Blank));
            i += 1;
            continue;
        }
        let mut r = Record::rec(i, Some((1, 0)));
        let n = owned(&f.values(i));
        i += 1;
        for _ in 0..n {
            if i >= f.len() {
                break;
            }
            r.children.push(Record::rec(i, Some((1, split))));
            i += 1;
        }
        out.push(r);
    }
    out
}

/// Schedules: a schedule's automatic tables come first, a name (and a plant) a line, with no
/// header of their own.
fn schedules(f: &TextFile) -> Vec<Record> {
    let mut out = owners(f, 3, |v| count(v, 1) + count(v, 2));
    for r in out.iter_mut().filter(|r| r.kind == Kind::Record) {
        let autos = count(&f.values(r.line), 2);
        for c in r.children.iter_mut().take(autos) {
            c.header = None;
        }
    }
    out
}

fn dtl(f: &TextFile) -> Vec<Record> {
    let mut out = Vec::new();
    for (i, k) in [(0, Kind::Title), (1, Kind::Count)] {
        if i < f.len() {
            out.push(Record::new(i, k));
        }
    }
    let mut i = 2;
    while i < f.len() {
        if blank(f, i) {
            out.push(Record::new(i, Kind::Blank));
            i += 1;
            continue;
        }
        // "name conds alts acts", then the table's line.
        let is_head = f.token(i, 0).is_some_and(|t| t.eq_ignore_ascii_case("name"));
        if !is_head || i + 1 >= f.len() {
            out.push(Record::new(i, Kind::Other));
            i += 1;
            continue;
        }
        out.push(Record::new(i, Kind::Header));
        let mut t = Record::rec(i + 1, Some((i, 0)));
        let v = f.values(i + 1);
        let (conds, acts) = (count(&v, 1), count(&v, 3));
        i += 2;
        for n in [conds, acts] {
            if i >= f.len() {
                break;
            }
            let h = i;
            t.children.push(Record::new(h, Kind::Header));
            i += 1;
            for _ in 0..n {
                if i >= f.len() {
                    break;
                }
                t.children.push(Record::rec(i, Some((h, 0))));
                i += 1;
            }
        }
        out.push(t);
    }
    out
}

fn wgn(f: &TextFile) -> Vec<Record> {
    let mut out = Vec::new();
    if !f.is_empty() {
        out.push(Record::new(0, Kind::Title));
    }
    let mut i = 1;
    while i < f.len() {
        if blank(f, i) {
            out.push(Record::new(i, Kind::Blank));
            i += 1;
            continue;
        }
        let mut s = Record::rec(i, None);
        i += 1;
        if i < f.len() {
            let h = i;
            s.children.push(Record::new(h, Kind::Header));
            i += 1;
            for _ in 0..12 {
                if i >= f.len() {
                    break;
                }
                s.children.push(Record::rec(i, Some((h, 0))));
                i += 1;
            }
        }
        out.push(s);
    }
    out
}

fn series(f: &TextFile) -> Vec<Record> {
    let mut out = Vec::new();
    for (i, k) in [(0, Kind::Title), (1, Kind::Header)] {
        if i < f.len() {
            out.push(Record::new(i, k));
        }
    }
    if f.len() > 2 {
        out.push(Record::rec(2, Some((1, 0))));
    }
    rows(f, 3, f.len(), None, &mut out);
    out
}
