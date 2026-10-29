//! A SWAT+ project (its TxtInOut folder) opened beside the map, for people and the AI assistant to
//! look at and edit through commands (A1 of the swatplus-rust roadmap): its summary, what is
//! missing, its parameters found by name and read, changed one at a time or many by a rule
//! (replace, add, or multiply, as SWAT-CUP's `v__`, `a__` and `r__`), checked against
//! `cal_parms.cal`'s ranges, what changed and undoing it, and saving the changed files.
//!
//! The project is held as its files' text (`rustgis_swat_model`), so saving writes back the same
//! bytes but for the values changed. It isn't part of the map project and isn't saved with it.
//!
//! A project opened as confidential (data that may not leave the computer) answers with counts
//! and statistics only, never a record's name or value; only the user can lift that
//! (`swat.confidential` is reserved to them).

use std::collections::{BTreeMap, BTreeSet};

use rustgis_swat_model::{Project, TextFile};
use rustgis_swat_txtinout::view::{Kind, View, columns};
use serde_json::{Value, json};

use super::{b, f, ps};
use crate::{CmdResult, CommandSpec as C, Session};

const WHERE: &str = "Analyze › SWAT+";

/// The most files, and bytes, a project may have: a large basin's weather files are big.
#[cfg(not(target_arch = "wasm32"))]
const MAX_FILES: usize = 20_000;
#[cfg(not(target_arch = "wasm32"))]
const MAX_BYTES: u64 = 4 << 30;
/// The most records a reply lists (the statistics cover them all).
const MAX_LISTED: usize = 500;

/// The open SWAT+ project.
#[derive(Clone, Debug)]
pub struct SwatState {
    /// The folder it was read from.
    pub dir: Option<std::path::PathBuf>,
    /// As read, and as edited.
    pub original: Project,
    pub project: Project,
    /// Answers carry statistics only.
    pub confidential: bool,
}

pub fn commands() -> Vec<C> {
    vec![
        C::new("swat.open", "Open SWAT+ Project", WHERE, open).view().params(
            r#"{"path": "a TxtInOut folder (with file.cio)", "confidential"?: bool (answers carry counts and statistics only, no record names or values; only the user can lift it)} — reads the project's input files"#,
        ),
        C::new("swat.close", "Close SWAT+ Project", WHERE, close).view().when(need_swat).params("{} — closes the SWAT+ project (unsaved changes are lost)"),
        C::new("swat.summary", "SWAT+ Project Summary", WHERE, summary)
            .view()
            .when(need_swat)
            .params("{} — the period simulated, the number of each kind of object, the files, and whether any are missing"),
        C::new("swat.check", "Check SWAT+ Project", WHERE, check).view().when(need_swat).params(
            r#"{} — input files file.cio names that are missing, and parameter values outside cal_parms.cal's calibration ranges (not necessarily wrong)"#,
        ),
        C::new("swat.find", "Find SWAT+ Parameter", WHERE, find).view().when(need_swat).params(
            r#"{"query": text (part of a column name, e.g. "cn", "esco", "awc"), "file"?: text (one file, e.g. "hydrology.hyd")} — the files and columns that match, with their records' count and range, and cal_parms.cal's range"#,
        ),
        C::new("swat.get", "Read SWAT+ Parameter", WHERE, get).view().when(need_swat).params(
            r#"{"file": text, "column": text, "match"?: "record names, * and ? as wildcards", "where"?: {"column": text, "match": "values, * and ? as wildcards"}} — the values of a column (up to 500 records listed) and their statistics"#,
        ),
        C::new("swat.set", "Change SWAT+ Parameter", WHERE, set).view().when(need_swat).params(
            r#"{"file": text, "column": text, "value": number or text, "mode"?: "replace" (default) | "add" | "multiply" (by 1 + value, as SWAT-CUP's r__), "match"?: record names with * and ?, "where"?: {"column": text, "match": text}, "force"?: bool (allow values outside cal_parms.cal's range)} — changes the matching records' values (in memory: swat.save writes them)"#,
        ),
        C::new("swat.diff", "SWAT+ Changes", WHERE, diff)
            .view()
            .when(need_swat)
            .params(r#"{"file"?: text} — every value changed since the project was opened (or counts only, when confidential)"#),
        C::new("swat.revert", "Revert SWAT+ Changes", WHERE, revert)
            .view()
            .when(need_swat)
            .params(r#"{"file"?: text, "column"?: text} — undoes the changes (all, a file's, or a file's column)"#),
        C::new("swat.save", "Save SWAT+ Project", WHERE, save).view().when(need_swat).params(
            r#"{"path"?: "a folder to save the whole project to (default: write the changed files over the folder it was opened from)"} — writes the files"#,
        ),
        C::new("swat.confidential", "SWAT+ Confidential Mode", WHERE, confidential)
            .view()
            .when(need_swat)
            .params(r#"{"on": bool} — answers with statistics only (on), or with record names and values (off; only the user can turn it off)"#),
    ]
}

fn need_swat(s: &Session) -> Option<&'static str> {
    if s.swat.is_none() { Some("open a SWAT+ project first (swat.open)") } else { None }
}

fn state(s: &Session) -> Result<&SwatState, String> {
    s.swat.as_deref().ok_or_else(|| "no SWAT+ project is open (swat.open)".to_string())
}

fn state_mut(s: &mut Session) -> Result<&mut SwatState, String> {
    s.swat.as_deref_mut().ok_or_else(|| "no SWAT+ project is open (swat.open)".to_string())
}

// ---------------------------------------------------------------------------------------------
// Opening

#[cfg(not(target_arch = "wasm32"))]
fn open(s: &mut Session, p: &Value) -> CmdResult {
    let dir = std::path::PathBuf::from(ps(p, "path").map(str::trim).filter(|d| !d.is_empty()).ok_or("give the TxtInOut folder's path")?);
    let project = read_folder(&dir)?;
    let st = SwatState { dir: Some(dir), original: project.clone(), project, confidential: b(p, "confidential").unwrap_or(false) };
    s.swat = Some(Box::new(st));
    summary(s, &json!({}))
}

#[cfg(target_arch = "wasm32")]
fn open(_: &mut Session, _: &Value) -> CmdResult {
    Err("SWAT+ projects open from a folder, in the desktop app".into())
}

/// Reads the files of the scenario in `dir`: `file.cio`, what it names, and what those name in
/// turn (weather files, recall files), until nothing new is named.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_folder(dir: &std::path::Path) -> Result<Project, String> {
    let read = |name: &str| -> Result<Option<Vec<u8>>, String> {
        let path = dir.join(name);
        match std::fs::metadata(&path) {
            Ok(m) if m.is_file() => std::fs::read(&path).map(Some).map_err(|e| format!("{name}: {e}")),
            _ => Ok(None),
        }
    };
    let cio = read("file.cio")?.ok_or_else(|| format!("{}: no file.cio here (a SWAT+ TxtInOut folder has one)", dir.display()))?;
    let mut project = rustgis_swat_txtinout::read([("file.cio", cio)]);
    let mut tried: BTreeSet<String> = BTreeSet::from(["file.cio".to_string()]);
    let mut bytes = 0u64;
    loop {
        let wanted: Vec<String> = rustgis_swat_txtinout::cio::inputs(&project).into_iter().filter(|n| !tried.contains(n)).collect();
        if wanted.is_empty() {
            break;
        }
        for name in wanted {
            tried.insert(name.clone());
            // A name stays inside the folder.
            if name.contains("..") || std::path::Path::new(&name).is_absolute() {
                continue;
            }
            if let Some(b) = read(&name)? {
                bytes += b.len() as u64;
                if bytes > MAX_BYTES || project.files.len() >= MAX_FILES {
                    return Err(format!(
                        "{}: the project is larger than this reads ({} files, {} MB)",
                        dir.display(),
                        project.files.len(),
                        bytes >> 20
                    ));
                }
                project.files.insert(name, rustgis_swat_txtinout::decode(&b));
            }
        }
    }
    Ok(project)
}

fn close(s: &mut Session, _: &Value) -> CmdResult {
    let st = s.swat.take().ok_or("no SWAT+ project is open")?;
    let changed = changed_files(&st).len();
    Ok(json!({"closed": true, "unsaved_files": changed}))
}

// ---------------------------------------------------------------------------------------------
// Reading

/// A table's records: (record line, values by column name).
struct Table<'a> {
    file: &'a TextFile,
    /// Each record's line and its columns' names.
    rows: Vec<(usize, Vec<String>)>,
}

fn table<'a>(p: &'a Project, name: &str) -> Option<Table<'a>> {
    let file = p.file(name)?;
    let view = View::of(name, file);
    let rows =
        view.flat().into_iter().filter(|r| r.kind == Kind::Record && r.header.is_some()).map(|r| (r.line, columns(file, r))).collect();
    Some(Table { file, rows })
}

impl Table<'_> {
    /// The value of `column` on a record's line, and its token's place.
    fn value(&self, line: usize, cols: &[String], column: &str) -> Option<(usize, &str)> {
        let k = cols.iter().position(|c| c.eq_ignore_ascii_case(column))?;
        Some((k, self.file.token(line, k)?))
    }

    fn name(&self, line: usize) -> &str {
        self.file.token(line, 0).unwrap_or("")
    }
}

/// `*` and `?` wildcards, without case.
fn glob(pattern: &str, text: &str) -> bool {
    fn go(p: &[char], t: &[char]) -> bool {
        match (p.first(), t.first()) {
            (None, None) => true,
            (Some('*'), _) => go(&p[1..], t) || (!t.is_empty() && go(p, &t[1..])),
            (Some('?'), Some(_)) => go(&p[1..], &t[1..]),
            (Some(a), Some(b)) if a.eq_ignore_ascii_case(b) => go(&p[1..], &t[1..]),
            _ => false,
        }
    }
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    go(&p, &t)
}

/// The records a command picks: by `match` on their names and `where` on another column.
fn picked(t: &Table, p: &Value) -> Result<Vec<usize>, String> {
    let name_pat = ps(p, "match");
    let cond = match p.get("where") {
        Some(w) if !w.is_null() => {
            let col = w.get("column").and_then(Value::as_str).ok_or("where needs a column")?;
            let pat = w.get("match").and_then(|v| v.as_str().map(str::to_string).or_else(|| (!v.is_null()).then(|| v.to_string())));
            Some((col.to_string(), pat.ok_or("where needs a match")?))
        }
        _ => None,
    };
    let mut out = Vec::new();
    for (i, (line, cols)) in t.rows.iter().enumerate() {
        if name_pat.is_some_and(|n| !glob(n, t.name(*line))) {
            continue;
        }
        if let Some((col, pat)) = &cond {
            match t.value(*line, cols, col) {
                Some((_, v)) if glob(pat, v) => {}
                _ => continue,
            }
        }
        out.push(i);
    }
    Ok(out)
}

/// Count, and for numbers their range, mean and how many differ.
fn stats<'a>(values: impl Iterator<Item = &'a str>) -> Value {
    let mut n = 0usize;
    let mut nums: Vec<f64> = Vec::new();
    let mut distinct = BTreeSet::new();
    for v in values {
        n += 1;
        if distinct.len() < 10_000 {
            distinct.insert(v.to_string());
        }
        if let Ok(x) = v.parse::<f64>()
            && x.is_finite()
        {
            nums.push(x);
        }
    }
    let mut out = json!({"count": n, "distinct": distinct.len()});
    if !nums.is_empty() {
        let min = nums.iter().copied().fold(f64::INFINITY, f64::min);
        let max = nums.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let mean = nums.iter().sum::<f64>() / nums.len() as f64;
        out["min"] = json!(min);
        out["max"] = json!(max);
        out["mean"] = json!(mean);
    }
    out
}

/// `cal_parms.cal`'s range of a parameter, by name.
fn range(p: &Project, column: &str) -> Option<(f64, f64)> {
    let t = table(p, "cal_parms.cal")?;
    t.rows.iter().find(|(l, _)| t.name(*l).eq_ignore_ascii_case(column)).and_then(|(l, cols)| {
        let lo = t.value(*l, cols, "abs_min")?.1.parse().ok()?;
        let hi = t.value(*l, cols, "abs_max")?.1.parse().ok()?;
        Some((lo, hi))
    })
}

fn summary(s: &mut Session, _: &Value) -> CmdResult {
    let st = state(s)?;
    let p = &st.project;
    let time = p.file("time.sim").map(|t| t.values(2).into_iter().map(str::to_string).collect::<Vec<_>>()).unwrap_or_default();
    let period = match time.as_slice() {
        [day_start, yrc_start, day_end, yrc_end, step, ..] => {
            json!({"first_day": day_start, "first_year": yrc_start, "last_day": day_end, "last_year": yrc_end, "steps_a_day": step})
        }
        _ => Value::Null,
    };
    // object.cnt: the number of each kind of object.
    let objects = table(p, "object.cnt")
        .and_then(|t| {
            let (line, cols) = t.rows.first()?;
            let mut o = serde_json::Map::new();
            for (k, c) in cols.iter().enumerate().skip(1) {
                if let Some(v) = t.file.token(*line, k).and_then(|v| v.parse::<f64>().ok()).filter(|v| *v > 0.0) {
                    o.insert(c.clone(), json!(v));
                }
            }
            Some(Value::Object(o))
        })
        .unwrap_or(Value::Null);
    let missing = rustgis_swat_txtinout::cio::missing(p);
    Ok(json!({
        "folder": st.dir.as_ref().map(|d| d.display().to_string()),
        "confidential": st.confidential,
        "period": period,
        "objects": objects,
        "files": p.files.len(),
        "missing_files": missing,
        "changed_files": changed_files(st),
    }))
}

fn check(s: &mut Session, _: &Value) -> CmdResult {
    let st = state(s)?;
    let p = &st.project;
    let missing = rustgis_swat_txtinout::cio::missing(p);
    // Every column with a range in cal_parms.cal, in every table.
    let ranges: BTreeMap<String, (f64, f64)> = match table(p, "cal_parms.cal") {
        Some(t) => t
            .rows
            .iter()
            .filter_map(|(l, cols)| {
                let lo = t.value(*l, cols, "abs_min")?.1.parse().ok()?;
                let hi = t.value(*l, cols, "abs_max")?.1.parse().ok()?;
                Some((t.name(*l).to_ascii_lowercase(), (lo, hi)))
            })
            .collect(),
        None => BTreeMap::new(),
    };
    let mut out_of_range = Vec::new();
    for name in p.files.keys() {
        if name == "cal_parms.cal" || name.contains('/') {
            continue;
        }
        let Some(t) = table(p, name) else { continue };
        let mut found: BTreeMap<String, (usize, f64, f64, (f64, f64))> = BTreeMap::new();
        for (line, cols) in &t.rows {
            for (k, c) in cols.iter().enumerate() {
                let Some(&(lo, hi)) = ranges.get(&c.to_ascii_lowercase()) else { continue };
                let Some(v) = t.file.token(*line, k).and_then(|v| v.parse::<f64>().ok()) else { continue };
                if v < lo || v > hi {
                    let e = found.entry(c.clone()).or_insert((0, f64::INFINITY, f64::NEG_INFINITY, (lo, hi)));
                    e.0 += 1;
                    e.1 = e.1.min(v);
                    e.2 = e.2.max(v);
                }
            }
        }
        for (c, (n, min, max, (lo, hi))) in found {
            out_of_range.push(json!({"file": name, "column": c, "records": n, "min": min, "max": max, "range": [lo, hi]}));
        }
    }
    // cal_parms.cal's ranges are calibration limits: a project's own values (the SWAT+ Editor's
    // defaults among them) can be outside them on purpose, so they're listed, not counted as faults.
    Ok(json!({
        "missing_files": missing,
        "outside_calibration_range": out_of_range,
        "note": "cal_parms.cal gives the ranges calibration may change a parameter within; values outside them are not necessarily wrong",
        "ok": missing.is_empty(),
    }))
}

fn find(s: &mut Session, p: &Value) -> CmdResult {
    let st = state(s)?;
    let q = ps(p, "query").map(str::trim).filter(|q| !q.is_empty()).ok_or("give a query (part of a column name)")?.to_ascii_lowercase();
    let only = ps(p, "file");
    let mut hits = Vec::new();
    for name in st.project.files.keys() {
        if only.is_some_and(|o| o != name) || name.contains('/') {
            continue;
        }
        let Some(t) = table(&st.project, name) else { continue };
        let mut cols: Vec<String> = Vec::new();
        for (_, c) in &t.rows {
            for col in c {
                if col.to_ascii_lowercase().contains(&q) && !cols.contains(col) {
                    cols.push(col.clone());
                }
            }
        }
        for col in cols {
            let values = t.rows.iter().filter_map(|(l, c)| t.value(*l, c, &col).map(|v| v.1));
            let mut hit = json!({"file": name, "column": col, "values": stats(values)});
            if let Some((lo, hi)) = range(&st.project, &col) {
                hit["range"] = json!([lo, hi]);
            }
            hits.push(hit);
            if hits.len() >= 200 {
                break;
            }
        }
    }
    Ok(json!({"query": q, "found": hits}))
}

fn get(s: &mut Session, p: &Value) -> CmdResult {
    let st = state(s)?;
    let (file, column) = file_column(p)?;
    let t = table(&st.project, file).ok_or_else(|| format!("no table {file} in the project"))?;
    let rows = picked(&t, p)?;
    let mut values = Vec::new();
    let mut listed = Vec::new();
    for &i in &rows {
        let (line, cols) = &t.rows[i];
        if let Some((_, v)) = t.value(*line, cols, column) {
            values.push(v);
            if listed.len() < MAX_LISTED {
                listed.push(json!({"name": t.name(*line), "value": v}));
            }
        }
    }
    if values.is_empty() && !rows.is_empty() {
        return Err(format!("{file} has no column {column}"));
    }
    let mut out = json!({"file": file, "column": column, "values": stats(values.iter().copied())});
    if let Some((lo, hi)) = range(&st.project, column) {
        out["range"] = json!([lo, hi]);
    }
    if st.confidential {
        out["records"] = json!("hidden: the project is confidential (statistics only)");
    } else {
        out["records"] = Value::Array(listed);
        out["more"] = json!(values.len().saturating_sub(MAX_LISTED));
    }
    Ok(out)
}

fn file_column(p: &Value) -> Result<(&str, &str), String> {
    let file = ps(p, "file").map(str::trim).filter(|f| !f.is_empty()).ok_or("give the file (e.g. hydrology.hyd)")?;
    let column = ps(p, "column").map(str::trim).filter(|c| !c.is_empty()).ok_or("give the column")?;
    Ok((file, column))
}

// ---------------------------------------------------------------------------------------------
// Changing

/// `x` written as the value it replaces is: with as many decimals, or in its exponent form.
fn format_like(old: &str, x: f64) -> String {
    if old.contains(['e', 'E']) {
        return format!("{x:e}");
    }
    match old.split_once('.') {
        Some((_, frac)) => format!("{x:.*}", frac.len()),
        None if old.parse::<i64>().is_ok() && x.fract() == 0.0 => format!("{}", x as i64),
        None => format!("{x}"),
    }
}

fn set(s: &mut Session, p: &Value) -> CmdResult {
    let st = state_mut(s)?;
    let (file, column) = file_column(p)?;
    let mode = ps(p, "mode").unwrap_or("replace");
    if !matches!(mode, "replace" | "add" | "multiply") {
        return Err(format!("mode is replace, add or multiply, not {mode}"));
    }
    let number = f(p, "value");
    let text = match p.get("value") {
        Some(Value::String(v)) => v.trim().to_string(),
        Some(v) if !v.is_null() => v.to_string(),
        _ => return Err("give the value".into()),
    };
    if mode != "replace" && number.is_none() {
        return Err(format!("{mode} needs a number"));
    }
    let range = range(&st.project, column);
    let force = b(p, "force").unwrap_or(false);
    // The changes, worked out before any is made.
    let mut edits: Vec<(usize, usize, String, String)> = Vec::new();
    {
        let t = table(&st.project, file).ok_or_else(|| format!("no table {file} in the project"))?;
        for i in picked(&t, p)? {
            let (line, cols) = &t.rows[i];
            let Some((k, old)) = t.value(*line, cols, column) else { continue };
            let new = match (mode, number, old.parse::<f64>()) {
                ("replace", Some(x), Ok(_)) => format_like(old, x),
                ("replace", _, _) => text.clone(),
                ("add", Some(x), Ok(o)) => format_like(old, o + x),
                ("multiply", Some(x), Ok(o)) => format_like(old, o * (1.0 + x)),
                _ => return Err(format!("{file} {column} of {} is {old}, not a number", t.name(*line))),
            };
            if let (Some((lo, hi)), Ok(v)) = (range, new.parse::<f64>())
                && (v < lo || v > hi)
                && !force
            {
                return Err(format!(
                    "{column} of {} would be {new}, outside cal_parms.cal's range {lo} to {hi} (give \"force\": true to set it anyway)",
                    t.name(*line)
                ));
            }
            edits.push((*line, k, old.to_string(), new));
        }
        if edits.is_empty() {
            return Err(format!("no record of {file} matches, or it has no column {column}"));
        }
    }
    let changed = edits.iter().filter(|(_, _, old, new)| old != new).count();
    let mut values = Vec::new();
    for (line, k, old, new) in &edits {
        st.project.set_token(file, *line, *k, new).map_err(|e| e.to_string())?;
        if values.len() < MAX_LISTED {
            values.push(json!({"line": line + 1, "from": old, "to": new}));
        }
    }
    let mut out = json!({"file": file, "column": column, "mode": mode, "records": edits.len(), "changed": changed});
    let after = table(&st.project, file)
        .map(|t| stats(t.rows.iter().filter_map(|(l, c)| t.value(*l, c, column).map(|v| v.1))))
        .unwrap_or(Value::Null);
    out["values_now"] = after;
    if !st.confidential {
        out["edits"] = Value::Array(values);
    }
    Ok(out)
}

/// The files whose text differs from what was read.
fn changed_files(st: &SwatState) -> Vec<String> {
    st.project.files.iter().filter(|(n, f)| st.original.file(n) != Some(*f)).map(|(n, _)| n.clone()).collect()
}

fn diff(s: &mut Session, p: &Value) -> CmdResult {
    let st = state(s)?;
    let only = ps(p, "file");
    let mut files = Vec::new();
    for name in changed_files(st) {
        if only.is_some_and(|o| o != name) {
            continue;
        }
        let (Some(new), Some(old)) = (st.project.file(&name), st.original.file(&name)) else { continue };
        let view = View::of(&name, new);
        let mut by_column: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for r in view.flat() {
            let cols = columns(new, r);
            let (a, b) = (old.values(r.line), new.values(r.line));
            for (k, (x, y)) in a.iter().zip(&b).enumerate() {
                if x != y {
                    let col = cols.get(k).cloned().unwrap_or_else(|| format!("col{}", k + 1));
                    by_column.entry(col).or_default().push(json!({"name": new.token(r.line, 0), "from": x, "to": y}));
                }
            }
        }
        let columns: Vec<Value> = by_column
            .into_iter()
            .map(|(c, v)| {
                if st.confidential {
                    json!({"column": c, "changed": v.len()})
                } else {
                    json!({"column": c, "changed": v.len(), "values": v.into_iter().take(MAX_LISTED).collect::<Vec<_>>()})
                }
            })
            .collect();
        files.push(json!({"file": name, "columns": columns}));
    }
    Ok(json!({"files": files}))
}

fn revert(s: &mut Session, p: &Value) -> CmdResult {
    let st = state_mut(s)?;
    let only = ps(p, "file");
    let column = ps(p, "column");
    let mut reverted = 0usize;
    for name in changed_files(st) {
        if only.is_some_and(|o| o != name) {
            continue;
        }
        let Some(old) = st.original.file(&name).cloned() else { continue };
        match column {
            None => {
                st.project.files.insert(name, old);
                reverted += 1;
            }
            Some(col) => {
                let Some(new) = st.project.file(&name).cloned() else { continue };
                let view = View::of(&name, &new);
                for r in view.flat() {
                    let cols = columns(&new, r);
                    let Some(k) = cols.iter().position(|c| c.eq_ignore_ascii_case(col)) else { continue };
                    if let Some(v) = old.token(r.line, k).filter(|v| Some(*v) != new.token(r.line, k)) {
                        st.project.set_token(&name, r.line, k, v).map_err(|e| e.to_string())?;
                        reverted += 1;
                    }
                }
            }
        }
    }
    Ok(json!({"reverted": reverted, "changed_files": changed_files(st)}))
}

#[cfg(not(target_arch = "wasm32"))]
fn save(s: &mut Session, p: &Value) -> CmdResult {
    let st = state_mut(s)?;
    let to = ps(p, "path").map(str::trim).filter(|d| !d.is_empty()).map(std::path::PathBuf::from);
    let (dir, names): (std::path::PathBuf, Vec<String>) = match to {
        // A folder of its own: the whole project.
        Some(d) => (d, st.project.files.keys().cloned().collect()),
        None => (st.dir.clone().ok_or("the project has no folder: give a path")?, changed_files(st)),
    };
    let mut written = 0usize;
    for name in &names {
        let Some(file) = st.project.file(name) else { continue };
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&path, rustgis_swat_txtinout::encode(file)).map_err(|e| format!("{}: {e}", path.display()))?;
        written += 1;
    }
    // What is saved over the folder it came from is the project as read from now on.
    if p.get("path").is_none_or(Value::is_null) {
        st.original = st.project.clone();
    }
    Ok(json!({"folder": dir.display().to_string(), "written": written}))
}

#[cfg(target_arch = "wasm32")]
fn save(_: &mut Session, _: &Value) -> CmdResult {
    Err("SWAT+ projects are saved to a folder, in the desktop app".into())
}

fn confidential(s: &mut Session, p: &Value) -> CmdResult {
    let st = state_mut(s)?;
    let on = b(p, "on").ok_or("give on: true or false")?;
    st.confidential = on;
    Ok(json!({"confidential": on}))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// A small scenario: file.cio naming three tables, one missing.
    fn scenario() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let w = |n: &str, t: &str| std::fs::write(d.path().join(n), t).unwrap();
        w(
            "file.cio",
            "file.cio: test\nsimulation        time.sim          print.prt         object.cnt\nhru               hru-data.hru      null\nhydrology         hydrology.hyd     null\nchg               cal_parms.cal     null\nsoils             soils.sol\n",
        );
        w("time.sim", "time.sim: test\nday_start  yrc_start  day_end  yrc_end  step\n       0       2015        0     2016     0\n");
        w("object.cnt", "object.cnt: test\nname  ls_area  tot_area  obj  hru  lhru  rtu\nbasin  100.0  100.0  3  2  0  1\n");
        w(
            "hru-data.hru",
            "hru-data.hru: test\nid  name  topo  hydro  soil  lu_mgt\n1  hru001  topohru001  hyd001  s1  agrl_lum\n2  hru002  topohru002  hyd002  s1  frst_lum\n",
        );
        w(
            "hydrology.hyd",
            "hydrology.hyd: test\nname           lat_ttime     lat_sed     can_max     esco     epco\nhyd001           0.00000     0.00000     1.00000  0.95000  1.00000\nhyd002           0.00000     0.00000     1.00000  0.80000  1.00000\n",
        );
        w(
            "cal_parms.cal",
            "cal_parms.cal: test\n2\nname  obj_typ  abs_min  abs_max  units\nesco  hru  0.000  1.000  null\nepco  hru  0.000  1.000  null\n",
        );
        d
    }

    fn session(d: &tempfile::TempDir, confidential: bool) -> Session {
        let mut s = Session::new(crate::Project::default());
        s.run("swat.open", &json!({"path": d.path().display().to_string(), "confidential": confidential})).unwrap();
        s
    }

    #[test]
    fn opens_and_summarises() {
        let d = scenario();
        let mut s = session(&d, false);
        let r = s.run("swat.summary", &json!({})).unwrap();
        assert_eq!(r["files"], 6);
        assert_eq!(r["missing_files"], json!(["print.prt", "soils.sol"]));
        assert_eq!(r["objects"]["hru"], 2.0);
        assert_eq!(r["period"]["first_year"], "2015");
        let c = s.run("swat.check", &json!({})).unwrap();
        assert_eq!(c["outside_calibration_range"], json!([]));
        assert_eq!(c["ok"], false);
    }

    #[test]
    fn finds_reads_and_changes_by_rule() {
        let d = scenario();
        let mut s = session(&d, false);
        let f = s.run("swat.find", &json!({"query": "esc"})).unwrap();
        assert_eq!(f["found"][0]["file"], "hydrology.hyd");
        assert_eq!(f["found"][0]["range"], json!([0.0, 1.0]));
        let g = s.run("swat.get", &json!({"file": "hydrology.hyd", "column": "esco"})).unwrap();
        assert_eq!(g["values"]["count"], 2);
        assert_eq!(g["records"][1], json!({"name": "hyd002", "value": "0.80000"}));
        // Multiply one record by 1 + 0.1, kept to the decimals it had.
        let r = s
            .run("swat.set", &json!({"file": "hydrology.hyd", "column": "esco", "value": 0.1, "mode": "multiply", "match": "hyd002"}))
            .unwrap();
        assert_eq!(r["changed"], 1);
        assert_eq!(r["edits"][0]["to"], "0.88000");
        // Out of range is refused, unless forced.
        assert!(s.run("swat.set", &json!({"file": "hydrology.hyd", "column": "esco", "value": 0.2, "mode": "add"})).is_err());
        let text = rustgis_swat_txtinout::encode(s.swat.as_ref().unwrap().project.file("hydrology.hyd").unwrap());
        assert!(String::from_utf8(text).unwrap().contains("  0.88000"), "only the value changes, the columns stay");
        let df = s.run("swat.diff", &json!({})).unwrap();
        assert_eq!(df["files"][0]["columns"][0]["values"][0], json!({"name": "hyd002", "from": "0.80000", "to": "0.88000"}));
        s.run("swat.revert", &json!({"file": "hydrology.hyd", "column": "esco"})).unwrap();
        assert_eq!(s.run("swat.diff", &json!({})).unwrap()["files"], json!([]));
    }

    #[test]
    fn where_picks_records_by_another_column() {
        let d = scenario();
        let mut s = session(&d, false);
        let g = s
            .run("swat.get", &json!({"file": "hru-data.hru", "column": "hydro", "where": {"column": "lu_mgt", "match": "agrl*"}}))
            .unwrap();
        assert_eq!(g["records"], json!([{"name": "1", "value": "hyd001"}]));
    }

    #[test]
    fn saving_writes_only_the_changed_values() {
        let d = scenario();
        let before = std::fs::read_to_string(d.path().join("hydrology.hyd")).unwrap();
        let mut s = session(&d, false);
        s.run("swat.set", &json!({"file": "hydrology.hyd", "column": "epco", "value": 0.9, "match": "hyd001"})).unwrap();
        let out = tempfile::tempdir().unwrap();
        let r = s.run("swat.save", &json!({"path": out.path().display().to_string()})).unwrap();
        assert_eq!(r["written"], 6);
        let after = std::fs::read_to_string(out.path().join("hydrology.hyd")).unwrap();
        let changed: Vec<(&str, &str)> = before.lines().zip(after.lines()).filter(|(a, b)| a != b).collect();
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].1.split_whitespace().last(), Some("0.90000"));
        assert_eq!(before.len(), after.len());
        // The other files are the same bytes.
        assert_eq!(std::fs::read(d.path().join("time.sim")).unwrap(), std::fs::read(out.path().join("time.sim")).unwrap());
    }

    #[test]
    fn a_confidential_project_answers_with_statistics_only() {
        let d = scenario();
        let mut s = session(&d, true);
        let g = s.run("swat.get", &json!({"file": "hydrology.hyd", "column": "esco"})).unwrap();
        assert_eq!(g["values"]["count"], 2);
        assert!(g["records"].is_string(), "{g}");
        assert!(!g.to_string().contains("hyd001"));
        let r = s.run("swat.set", &json!({"file": "hydrology.hyd", "column": "esco", "value": 0.9})).unwrap();
        assert!(r.get("edits").is_none());
        let df = s.run("swat.diff", &json!({})).unwrap();
        assert!(!df.to_string().contains("0.90000") && !df.to_string().contains("hyd00"), "{df}");
        // Only the user may lift it.
        assert!(super::super::plugins::user_only("swat.confidential"));
    }

    #[test]
    fn globbing() {
        assert!(glob("hru*", "HRU0012"));
        assert!(glob("h?d00?", "hyd002"));
        assert!(!glob("frst*", "agrl_lum"));
    }
}
