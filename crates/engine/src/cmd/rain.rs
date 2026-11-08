//! Rain grids (GSMaP and other rain rate or rain amount rasters): drawn in BMKG's intensity
//! classes, summed over a period, and averaged over zones such as a SWAT+ model's subbasins.
//!
//! - `rain.style`: classes of rain intensity, hourly (mm/h) or daily (mm a day), as BMKG uses them,
//!   or breaks of one's own; rain below the lightest class is not drawn.
//! - `rain.accumulate`: the rain of several hourly grids added up (mm over the period). A grid
//!   whose name gives its start time (as GSMaP's layers do: `GSMaP_NOW 2025-11-27 06:00 UTC`) is
//!   counted for the time to the next one, so GSMaP_NOW's half-hourly files, each an hour's mean
//!   rate, aren't counted twice.
//! - `rain.zonal`: each zone's mean rain from each grid, weighted by the share of each cell the
//!   zone covers (a GSMaP cell is about 11 km, larger than many subbasins), as a table.

use std::sync::Arc;

use rustgis_data::raster::Raster;
use rustgis_data::{Dataset, Field, FieldType, Geometry, GeometryType, Value as DValue};
use rustgis_map::color::{CustomRamp, RampKind};
use rustgis_map::symbology::ClassMethod;
use rustgis_map::{Ramp, RasterRenderer, Rgba};
use serde_json::{Value, json};

use super::{f, ps};
use crate::{CmdResult, CommandSpec as C, Session};

const WHERE: &str = "Analyze › Rain";

/// BMKG's rain intensity classes: the upper bound of each (mm/h or mm a day) and its name. The
/// lightest class starts at the first threshold; below it nothing is drawn.
const HOURLY: (f64, [(f64, &str); 4]) = (1.0, [(5.0, "ringan"), (10.0, "sedang"), (20.0, "lebat"), (f64::INFINITY, "sangat lebat")]);
const DAILY: (f64, [(f64, &str); 5]) =
    (0.5, [(20.0, "ringan"), (50.0, "sedang"), (100.0, "lebat"), (150.0, "sangat lebat"), (f64::INFINITY, "ekstrem")]);

/// The classes' colours, lightest first: light blue, blue, yellow, orange, dark red. Four classes
/// (hourly) leave out the yellow, so the heaviest is still dark red.
const COLOURS: [Rgba; 5] = [
    Rgba::rgb(0x9e, 0xca, 0xe1),
    Rgba::rgb(0x31, 0x82, 0xbd),
    Rgba::rgb(0xfe, 0xc4, 0x4f),
    Rgba::rgb(0xe6, 0x55, 0x0d),
    Rgba::rgb(0xa5, 0x0f, 0x15),
];

/// The colours of `n` classes (1 to 5), lightest first.
fn colours_for(n: usize) -> Vec<Rgba> {
    let pick: &[usize] = match n {
        1 => &[4],
        2 => &[1, 4],
        3 => &[0, 3, 4],
        4 => &[0, 1, 3, 4],
        _ => &[0, 1, 2, 3, 4],
    };
    pick.iter().map(|&i| COLOURS[i]).collect()
}

pub fn commands() -> Vec<C> {
    vec![
        C::new("rain.style", "Rain Intensity Classes", WHERE, style).when(super::raster::need_raster).params(
            r#"{"layer"?, "scale"?: "hourly" (mm/h, default for a rain rate) | "daily" (mm a day, default for 24 h or more) | "custom", "breaks"?: [numbers] (custom: the lightest class's lower bound, then each class's upper bound), "band"?: 1..} — draws a rain grid in BMKG's intensity classes: hourly ringan 1–5, sedang 5–10, lebat 10–20, sangat lebat over 20 mm/h; daily ringan 0.5–20, sedang 20–50, lebat 50–100, sangat lebat 100–150, ekstrem over 150 mm; less is not drawn"#,
        ),
        C::new("rain.accumulate", "Accumulate Rain", WHERE, accumulate).params(
            r#"{"layers": [rain rate layers (mm/h) on one grid], "hours"?: number (each grid's share when the names don't give times; default 1), "ignoreMissing"?: bool (a cell missing in one grid counts as no rain there, default false: missing), "name"?: text} — adds up hourly rain grids into the rain over the period (mm), as a new layer drawn in rain classes"#,
        ),
        C::new("rain.zonal", "Rain per Zone", WHERE, zonal).params(
            r#"{"layers": [rain layers], "zones": polygon layer (e.g. subbasins), "field"?: the zones' id field (default: their row number), "name"?: output table} — each zone's mean rain from each layer, weighted by the share of each cell the zone covers, as a table (zone, layer, time, mean, coverage)"#,
        ),
    ]
}

/// A raster layer's id and pixels, by `layer` (or the active layer).
fn get(s: &Session, p: &Value) -> Result<(u32, Arc<Raster>), String> {
    let id = s.layer_id(p)?;
    let l = s.map().layer(id).ok_or("no such layer")?;
    let rl = l.raster.as_ref().ok_or_else(|| format!("{} isn't a raster layer", l.name))?;
    Ok((id, rl.data.clone().ok_or_else(|| format!("{}: the raster's data couldn't be read", l.name))?))
}

fn raster_named(s: &Session, n: &str) -> Result<(String, Arc<Raster>), String> {
    let l = s.map().layer_by_name(n).ok_or_else(|| format!("no layer named '{n}'"))?;
    let r = l.raster.as_ref().and_then(|r| r.data.clone()).ok_or_else(|| format!("{n} isn't a raster layer"))?;
    Ok((l.name.clone(), r))
}

/// The class breaks and colours of a scale: the renderer's breaks (the first, below the lightest
/// class, drawn clear) and the classes' names with their bounds.
fn classes(scale: &str, custom: Option<&[f64]>, top: f64) -> Result<(Vec<f64>, Vec<Rgba>, Vec<Value>), String> {
    let (lowest, bounds): (f64, Vec<(f64, String)>) = match scale {
        "hourly" => (HOURLY.0, HOURLY.1.iter().map(|(b, n)| (*b, n.to_string())).collect()),
        "daily" => (DAILY.0, DAILY.1.iter().map(|(b, n)| (*b, n.to_string())).collect()),
        "custom" => {
            let b = custom.ok_or("custom classes need breaks")?;
            if b.len() < 2 || b.len() > COLOURS.len() + 1 || b.windows(2).any(|w| w[1] <= w[0]) || b.iter().any(|v| !v.is_finite()) {
                return Err(format!("breaks are 2 to {} increasing numbers", COLOURS.len() + 1));
            }
            let n = b.len() - 1;
            (
                b[0],
                b[1..].iter().enumerate().map(|(i, v)| (if i + 1 == n { f64::INFINITY } else { *v }, format!("class {}", i + 1))).collect(),
            )
        }
        other => return Err(format!("scale is hourly, daily or custom, not {other}")),
    };
    let n = bounds.len();
    // The top class reaches past the largest value, so every cell falls in a class.
    let top = top.max(bounds.iter().rev().nth(1).map_or(lowest, |b| b.0) * 2.0);
    let mut breaks = vec![lowest];
    breaks.extend(bounds.iter().map(|(b, _)| if b.is_finite() { *b } else { top }));
    let mut colours = vec![Rgba::new(0, 0, 0, 0)];
    colours.extend(colours_for(n));
    let mut lo = lowest;
    let named = bounds
        .iter()
        .map(|(b, name)| {
            let v = if b.is_finite() { json!({"class": name, "from": lo, "to": b}) } else { json!({"class": name, "from": lo}) };
            lo = *b;
            v
        })
        .collect();
    Ok((breaks, colours, named))
}

/// Whether a raster's values are amounts over a day or more (mm), from its band's name as
/// `rain.accumulate` writes it.
fn is_daily(r: &Raster) -> bool {
    r.bands.first().is_some_and(|b| b.name.contains(" h)") && hours_in(&b.name).is_some_and(|h| h >= 24.0))
}

fn hours_in(band: &str) -> Option<f64> {
    let inner = band.rsplit_once(", ")?.1.trim_end_matches(" h)");
    inner.parse().ok()
}

fn style(s: &mut Session, p: &Value) -> CmdResult {
    let (id, r) = get(s, p)?;
    let band = f(p, "band").map(|b| b as usize).unwrap_or(1);
    if band == 0 || band > r.band_count() {
        return Err(format!("{} has bands 1 to {}", r.name, r.band_count()));
    }
    let scale = ps(p, "scale").map(str::to_string).unwrap_or_else(|| if is_daily(&r) { "daily".into() } else { "hourly".into() });
    let custom: Option<Vec<f64>> = p.get("breaks").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_f64).collect());
    let top = r.bands[band - 1].stats.max;
    let (breaks, colours, named) = classes(&scale, custom.as_deref(), if top.is_finite() { top } else { 0.0 })?;
    let ramp = CustomRamp::new(&format!("Rain {scale}"), RampKind::Qualitative, colours.iter().map(|c| (None, *c)).collect())?;
    let n = breaks.len();
    let l = s.map_mut().layer_mut(id).ok_or("no such layer")?;
    let rl = l.raster.as_mut().ok_or("not a raster layer")?;
    rl.style.renderer =
        RasterRenderer::Classify { band: band - 1, method: ClassMethod::Manual, ramp: Ramp::Custom(Box::new(ramp)), breaks, classes: n };
    Ok(json!({"layer": id, "scale": scale, "classes": named, "units": if scale == "daily" { "mm" } else { "mm/h" }}))
}

// ---------------------------------------------------------------------------------------------
// Time from a layer's name

/// The start time a layer's name gives, as minutes since 1970 (UTC): `… YYYY-MM-DD HH:MM …`.
fn start_minutes(name: &str) -> Option<i64> {
    let b = name.as_bytes();
    (0..b.len().saturating_sub(15)).find_map(|i| {
        let s = name.get(i..i + 16)?;
        let d = |r: std::ops::Range<usize>| s.get(r).filter(|t| t.bytes().all(|c| c.is_ascii_digit())).and_then(|t| t.parse::<i64>().ok());
        if s.as_bytes()[4] != b'-' || s.as_bytes()[7] != b'-' || s.as_bytes()[10] != b' ' || s.as_bytes()[13] != b':' {
            return None;
        }
        let (y, m, day, h, mi) = (d(0..4)?, d(5..7)?, d(8..10)?, d(11..13)?, d(14..16)?);
        if !(1..=12).contains(&m) || !(1..=31).contains(&day) || h > 23 || mi > 59 {
            return None;
        }
        Some(days_from_civil(y, m, day) * 1440 + h * 60 + mi)
    })
}

/// Days since 1970-01-01 of a Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil(minutes: i64) -> String {
    let z = minutes.div_euclid(1440) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let t = minutes.rem_euclid(1440);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", t / 60, t % 60)
}

/// The product named before the time (`GSMaP_NOW 2025-…` → `GSMaP_NOW`).
fn product(name: &str) -> &str {
    name.split(' ').next().unwrap_or(name)
}

// ---------------------------------------------------------------------------------------------
// Accumulating

fn accumulate(s: &mut Session, p: &Value) -> CmdResult {
    let names: Vec<String> = p
        .get("layers")
        .and_then(Value::as_array)
        .ok_or("layers is a list of rain rate layers")?
        .iter()
        .map(|v| v.as_str().map(str::to_string).ok_or("each layer is a name"))
        .collect::<Result<_, _>>()?;
    if names.is_empty() || names.len() > 10_000 {
        return Err("give 1 to 10000 layers".into());
    }
    let mut grids: Vec<(String, Arc<Raster>, Option<i64>)> = Vec::new();
    for n in &names {
        let (name, r) = raster_named(s, n)?;
        let t = start_minutes(&name);
        grids.push((name, r, t));
    }
    let first = grids[0].1.clone();
    for (n, r, _) in &grids {
        if (r.width, r.height) != (first.width, first.height) || r.transform != first.transform || r.crs != first.crs {
            return Err(format!("{n} isn't on the same grid as {}", grids[0].0));
        }
    }
    // Each grid counts for the hours until the next one starts (the last, for the usual step);
    // without times, for `hours` each.
    let timed = grids.iter().all(|g| g.2.is_some());
    let mut weights: Vec<f64> = vec![f(p, "hours").unwrap_or(1.0); grids.len()];
    let mut note = None;
    let (mut start, mut end) = (None, None);
    if timed && grids.len() > 1 {
        grids.sort_by_key(|g| g.2);
        let t: Vec<i64> = grids.iter().filter_map(|g| g.2).collect();
        if t.windows(2).any(|w| w[1] == w[0]) {
            return Err("two grids start at the same time".into());
        }
        let steps: Vec<i64> = t.windows(2).map(|w| w[1] - w[0]).collect();
        let usual = {
            let mut v = steps.clone();
            v.sort_unstable();
            v[v.len() / 2]
        };
        for (k, w) in weights.iter_mut().enumerate() {
            let step = steps.get(k).copied().unwrap_or(usual);
            *w = (step.min(60) as f64) / 60.0;
        }
        if steps.iter().any(|st| *st > 60) {
            note = Some(format!(
                "there are gaps between the grids (up to {} h): the rain in them isn't counted",
                steps.iter().max().copied().unwrap_or(0) as f64 / 60.0
            ));
        }
        start = t.first().copied();
        end = t.last().map(|l| l + usual.min(60));
    } else if timed {
        start = grids[0].2;
        end = start.map(|s| s + (weights[0] * 60.0) as i64);
    }
    let ignore = super::b(p, "ignoreMissing").unwrap_or(false);
    let n = (first.width as usize) * (first.height as usize);
    let mut sum = vec![0f32; n];
    let mut missing = vec![false; n];
    for ((_, r, _), w) in grids.iter().zip(&weights) {
        let data = &r.bands.first().ok_or("a layer has no band")?.data;
        for i in 0..n {
            let v = data[i];
            if r.is_valid(v) {
                sum[i] += v * *w as f32;
            } else if !ignore {
                missing[i] = true;
            }
        }
    }
    for (v, m) in sum.iter_mut().zip(&missing) {
        if *m {
            *v = rustgis_analysis::rasterops::NODATA;
        }
    }
    let hours: f64 = weights.iter().sum();
    let label = match (start, end) {
        (Some(a), Some(b)) => format!("{} {}–{} UTC ({} h)", product(&grids[0].0), civil(a), &civil(b)[11..], fmt_hours(hours)),
        _ => format!("Rain ({} h)", fmt_hours(hours)),
    };
    let name = super::imagery::name(p, label);
    let mut out = rustgis_analysis::rasterops::derive(&first, &name, vec![sum])?;
    if let Some(b) = out.bands.first_mut() {
        b.name = format!("Rain (mm, {} h)", fmt_hours(hours));
    }
    let v = super::imagery::add(s, out, false)?;
    let id = v["layer"].as_u64().unwrap_or(0) as u32;
    style(s, &json!({"layer": id, "scale": if hours >= 24.0 { "daily" } else { "hourly" }}))?;
    let mut res = json!({"layer": id, "name": name, "grids": grids.len(), "hours": hours, "units": "mm"});
    if let Some(nt) = note {
        res["note"] = json!(nt);
    }
    if hours < 24.0 {
        res["classes_note"] =
            json!("drawn in the hourly classes (mm/h); for amounts over a few hours give rain.style a scale or breaks of your own");
    }
    Ok(res)
}

fn fmt_hours(h: f64) -> String {
    if h.fract() == 0.0 { format!("{h:.0}") } else { format!("{h:.1}") }
}

// ---------------------------------------------------------------------------------------------
// Zones

/// Samples taken across each cell to find the share of it a zone covers.
const SUB: usize = 8;

/// The mean of `r` over the polygon `rings` (lon/lat), each cell weighted by the share of it inside,
/// and that share's sum over the cells with values against all the zone's cells (coverage, 0–1).
/// `None` when the zone covers no cell of the grid.
pub fn zone_mean(r: &Raster, band: usize, polys: &[Vec<Vec<rustgis_data::Coord>>]) -> Option<(f64, f64)> {
    let mut e = rustgis_data::Extent::EMPTY;
    for rings in polys {
        for c in rings.iter().flatten() {
            e.expand(*c);
        }
    }
    if e.is_empty() {
        return None;
    }
    // Only the cells under the zone's bounds (a north-up longitude/latitude grid, as rain grids are).
    let (c0, r0) = r.transform.to_pixel(rustgis_data::Coord::new(e.min_x, e.max_y));
    let (c1, r1) = r.transform.to_pixel(rustgis_data::Coord::new(e.max_x, e.min_y));
    let (cmin, cmax) = (c0.min(c1).floor().max(0.0) as i64, c0.max(c1).ceil().min(r.width as f64) as i64);
    let (rmin, rmax) = (r0.min(r1).floor().max(0.0) as i64, r0.max(r1).ceil().min(r.height as f64) as i64);
    let (mut sum, mut wsum, mut all) = (0.0f64, 0.0f64, 0.0f64);
    for row in rmin..rmax {
        for col in cmin..cmax {
            let mut inside = 0usize;
            for i in 0..SUB {
                for j in 0..SUB {
                    let w = r.transform.to_world(col as f64 + (j as f64 + 0.5) / SUB as f64, row as f64 + (i as f64 + 0.5) / SUB as f64);
                    if polys.iter().any(|rings| rustgis_data::geometry::polygon_contains(rings, w)) {
                        inside += 1;
                    }
                }
            }
            if inside == 0 {
                continue;
            }
            let share = inside as f64 / (SUB * SUB) as f64;
            all += share;
            if let Some(v) = r.value(band, col, row) {
                sum += f64::from(v) * share;
                wsum += share;
            }
        }
    }
    if all == 0.0 {
        // A zone too small for any sample: the cell under its middle.
        let mid = rustgis_data::Coord::new((e.min_x + e.max_x) / 2.0, (e.min_y + e.max_y) / 2.0);
        let (col, row) = r.pixel_at(mid)?;
        return r.value(band, col, row).map(|v| (f64::from(v), 1.0));
    }
    (wsum > 0.0).then(|| (sum / wsum, wsum / all))
}

fn zonal(s: &mut Session, p: &Value) -> CmdResult {
    let names: Vec<String> = p
        .get("layers")
        .and_then(Value::as_array)
        .ok_or("layers is a list of rain layers")?
        .iter()
        .map(|v| v.as_str().map(str::to_string).ok_or("each layer is a name"))
        .collect::<Result<_, _>>()?;
    let zname = ps(p, "zones").ok_or("zones (a polygon layer) is required")?;
    let zl = s
        .map()
        .layer_by_name(zname)
        .filter(|l| l.geometry_type() == GeometryType::Polygon)
        .ok_or_else(|| format!("no polygon layer '{zname}'"))?;
    let field = ps(p, "field").map(|f| zl.dataset.field_index(f).ok_or_else(|| format!("{zname} has no field {f}"))).transpose()?;
    let zones: Vec<(String, Vec<Vec<Vec<rustgis_data::Coord>>>)> = zl
        .dataset
        .features
        .iter()
        .enumerate()
        .filter_map(|(row, ft)| match &ft.geometry {
            Some(Geometry::Polygon(p)) => {
                let id = field.map(|fi| zl.dataset.value(row, fi).to_string()).unwrap_or_else(|| (row + 1).to_string());
                Some((id, p.clone()))
            }
            _ => None,
        })
        .collect();
    if zones.is_empty() {
        return Err(format!("{zname} has no polygons"));
    }
    let mut ds = Dataset::new(
        super::imagery::name(p, format!("Rain per zone ({zname})")),
        GeometryType::None,
        vec![
            Field::new("zone", FieldType::Text),
            Field::new("layer", FieldType::Text),
            Field::new("time", FieldType::Text),
            Field::new("mean", FieldType::Double),
            Field::new("units", FieldType::Text),
            Field::new("coverage", FieldType::Double),
        ],
    );
    let mut none = 0usize;
    for n in &names {
        let (name, r) = raster_named(s, n)?;
        let units = if r.bands.first().is_some_and(|b| b.name.starts_with("Rain (mm,")) { "mm" } else { "mm/h" };
        let time = start_minutes(&name).map(|t| format!("{} UTC", civil(t))).unwrap_or_default();
        for (zid, polys) in &zones {
            let (mean, cov) = match zone_mean(&r, 0, polys) {
                Some((m, c)) => (DValue::Float(m), DValue::Float(c)),
                None => {
                    none += 1;
                    (DValue::Null, DValue::Float(0.0))
                }
            };
            ds.push_next(
                None,
                vec![
                    DValue::Text(zid.clone()),
                    DValue::Text(name.clone()),
                    DValue::Text(time.clone()),
                    mean,
                    DValue::Text(units.into()),
                    cov,
                ],
            );
        }
    }
    let rows = ds.features.len();
    let table = s.project.put_table(ds);
    let mut out = json!({"table": table, "rows": rows, "zones": zones.len(), "layers": names.len()});
    if none > 0 {
        out["no_rain_data"] = json!(none);
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rustgis_data::Coord;
    use rustgis_data::raster::{ColorHint, Crs, GeoTransform};

    /// A 0.1° grid over 99–102° E, 2° S–1° N, every cell `v`, one cell missing.
    fn grid(name: &str, v: f32) -> Raster {
        let (w, h) = (30u32, 30u32);
        let mut data = vec![v; (w * h) as usize];
        data[0] = f32::NAN;
        let t = GeoTransform { origin_x: 99.0, origin_y: 1.0, pixel_width: 0.1, pixel_height: -0.1 };
        let mut r = Raster::new(name, w, h, vec![data], t, Crs::LonLat, None, ColorHint::Gray, "Float32").unwrap();
        r.bands[0].name = "Rain rate (mm/h)".into();
        r
    }

    fn session(grids: &[Raster]) -> Session {
        let mut s = Session::new(crate::Project::default());
        for g in grids {
            super::super::imagery::add(&mut s, g.clone(), false).unwrap();
        }
        s
    }

    #[test]
    fn times_from_names() {
        let t = start_minutes("GSMaP_NOW 2025-11-27 06:30 UTC").unwrap();
        assert_eq!(civil(t), "2025-11-27 06:30");
        assert_eq!(start_minutes("GSMaP_NOW 2025-11-28 00:00 UTC").unwrap() - t, 17 * 60 + 30);
        assert_eq!(start_minutes("rain"), None);
        assert_eq!(civil(days_from_civil(2024, 2, 29) * 1440), "2024-02-29 00:00");
    }

    #[test]
    fn half_hourly_grids_count_half_an_hour_each() {
        let names = ["GSMaP_NOW 2025-11-27 06:00 UTC", "GSMaP_NOW 2025-11-27 06:30 UTC", "GSMaP_NOW 2025-11-27 07:00 UTC"];
        let mut s = session(&[grid(names[0], 2.0), grid(names[1], 4.0), grid(names[2], 6.0)]);
        let r = s.run("rain.accumulate", &json!({"layers": names})).unwrap();
        assert_eq!(r["hours"], 1.5);
        assert_eq!(r["name"], "GSMaP_NOW 2025-11-27 06:00–07:30 UTC (1.5 h)");
        let (_, out) = raster_named(&s, r["name"].as_str().unwrap()).unwrap();
        let (c, row) = out.pixel_at(Coord::new(100.05, -0.05)).unwrap();
        // (2 + 4 + 6) × 0.5 h.
        assert_eq!(out.value(0, c, row), Some(6.0));
        assert_eq!(out.value(0, 0, 0), None, "missing in one grid is missing");
        assert_eq!(out.bands[0].name, "Rain (mm, 1.5 h)");
    }

    #[test]
    fn hourly_grids_and_untimed_ones() {
        let mut s = session(&[
            grid("GSMaP_NOW 2025-11-27 06:00 UTC", 2.0),
            grid("GSMaP_NOW 2025-11-27 07:00 UTC", 3.0),
            grid("a", 1.0),
            grid("b", 1.0),
        ]);
        let r = s.run("rain.accumulate", &json!({"layers": ["GSMaP_NOW 2025-11-27 07:00 UTC", "GSMaP_NOW 2025-11-27 06:00 UTC"]})).unwrap();
        assert_eq!(r["hours"], 2.0);
        let (_, out) = raster_named(&s, r["name"].as_str().unwrap()).unwrap();
        assert_eq!(out.value(0, 5, 5), Some(5.0));
        let r = s.run("rain.accumulate", &json!({"layers": ["a", "b"], "hours": 3, "ignoreMissing": true})).unwrap();
        let (_, out) = raster_named(&s, r["name"].as_str().unwrap()).unwrap();
        assert_eq!(out.value(0, 0, 0), Some(0.0), "missing counts as no rain when asked");
        assert_eq!(out.value(0, 5, 5), Some(6.0));
        assert!(s.run("rain.accumulate", &json!({"layers": ["a", "a"]})).is_ok(), "untimed grids may repeat");
    }

    #[test]
    fn classes_are_bmkg_s() {
        let (b, c, named) = classes("hourly", None, 60.0).unwrap();
        assert_eq!(b, vec![1.0, 5.0, 10.0, 20.0, 60.0]);
        assert_eq!(c[0].0[3], 0, "below the lightest class is clear");
        assert_eq!((c[1], c[4]), (COLOURS[0], COLOURS[4]), "ringan light blue, sangat lebat dark red");
        assert_eq!(named[3], json!({"class": "sangat lebat", "from": 20.0}));
        let (b, _, named) = classes("daily", None, 10.0).unwrap();
        assert_eq!(b, vec![0.5, 20.0, 50.0, 100.0, 150.0, 300.0]);
        assert_eq!(named[4]["class"], "ekstrem");
        assert!(classes("custom", Some(&[1.0, 1.0]), 1.0).is_err());
        let mut s = session(&[grid("x", 12.0)]);
        let r = s.run("rain.style", &json!({"layer": "x"})).unwrap();
        assert_eq!(r["scale"], "hourly");
        let l = s.map().layer_by_name("x").unwrap();
        let RasterRenderer::Classify { breaks, method, .. } = &l.raster.as_ref().unwrap().style.renderer else { panic!() };
        assert_eq!((breaks.len(), *method), (5, ClassMethod::Manual));
    }

    #[test]
    fn a_zone_s_mean_weights_the_cells_it_covers() {
        let r = grid("x", 0.0);
        // Cells 99.0–99.1 rain 10, 99.1–99.2 rain 20 (row of 0.9–1.0° N).
        let mut r2 = r.clone();
        for row in 0..30 {
            r2.bands[0].data[row * 30] = 10.0;
            r2.bands[0].data[row * 30 + 1] = 20.0;
        }
        // A zone over 99.05–99.15, 0.5–0.7: half of each column.
        let z = vec![vec![vec![
            Coord::new(99.05, 0.5),
            Coord::new(99.15, 0.5),
            Coord::new(99.15, 0.7),
            Coord::new(99.05, 0.7),
            Coord::new(99.05, 0.5),
        ]]];
        let (m, cov) = zone_mean(&r2, 0, &z).unwrap();
        assert!((m - 15.0).abs() < 1e-9, "{m}");
        assert!((cov - 1.0).abs() < 1e-9);
        // A zone far smaller than a cell still gets its cell's rain.
        let tiny = vec![vec![vec![Coord::new(99.01, 0.51), Coord::new(99.03, 0.51), Coord::new(99.03, 0.53), Coord::new(99.01, 0.51)]]];
        assert_eq!(zone_mean(&r2, 0, &tiny).map(|v| v.0), Some(10.0));
        let mut s = session(&[r2.clone()]);
        let mut zones = Dataset::new("subbasins", GeometryType::Polygon, vec![Field::new("Subbasin", FieldType::Integer)]);
        zones.push(Some(Geometry::Polygon(z.clone())), vec![DValue::Int(7)]);
        super::super::layer::add_dataset(&mut s, zones);
        let out = s.run("rain.zonal", &json!({"layers": ["x"], "zones": "subbasins", "field": "Subbasin", "name": "rain"})).unwrap();
        assert_eq!(out["rows"], 1);
        let t = s.project.tables.iter().find(|t| t.name == "rain").unwrap();
        assert_eq!(t.value(0, 0), &DValue::Text("7".into()));
        assert_eq!(t.value(0, 3), &DValue::Float(15.0));
        // Off the grid: none.
        let off = vec![vec![vec![Coord::new(120.0, 0.0), Coord::new(121.0, 0.0), Coord::new(121.0, 1.0), Coord::new(120.0, 0.0)]]];
        assert_eq!(zone_mean(&r2, 0, &off), None);
    }
}
