//! PostGIS: connect to a PostgreSQL database with PostGIS, list its tables and add them as layers
//! that can be edited (desktop only). The ribbon's PostGIS tab runs these.
//!
//! The connection is the session's (`Session::postgis`), never the project's, and never with its
//! password, which is remembered for the session only. A table added with `pg.addLayer` is loaded
//! whole, its feature ids its integer primary key, and the layer keeps each row's version
//! (`xmin`) as read, so saving can tell a row someone else changed since from one to overwrite.

use serde_json::Value;
#[cfg(not(target_arch = "wasm32"))]
use serde_json::json;

#[cfg(not(target_arch = "wasm32"))]
use super::{f, ps};
use crate::{CmdResult, CommandSpec as C, Session};

/// Why a command needs a connection first.
const NOT_CONNECTED: &str = "connect to a PostGIS database first (PostGIS › Connect PostGIS)";

/// `sslmode`s a connection may name, as libpq's.
#[cfg(not(target_arch = "wasm32"))]
const SSL_MODES: &[&str] = &["disable", "allow", "prefer", "require", "verify-ca", "verify-full"];

/// The most tables `pg.tables` lists.
#[cfg(not(target_arch = "wasm32"))]
const MAX_LISTED: usize = 5_000;

pub fn commands() -> Vec<C> {
    vec![
        C::new("pg.connect", "Connect PostGIS", "PostGIS › Connection", connect).view().params(
            r#"{"connection"?: "host=… port=… dbname=… user=…, or a postgresql:// address (instead of the parts below)", "host"?: string, "port"?: number, "database"?: string, "user"?: string, "password"?: "kept for this session only, never saved", "sslmode"?: "disable"|"prefer"|"require"|"verify-ca"|"verify-full"}"#,
        ),
        C::new("pg.disconnect", "Disconnect PostGIS", "PostGIS › Connection", disconnect).view().when(need_connection),
        C::new("pg.status", "PostGIS Connection", "PostGIS › Connection", status).view(),
        C::new("pg.tables", "List PostGIS Tables", "PostGIS › Layer", tables).view().when(need_connection).params(r#"{"schema"?: string}"#),
        C::new("pg.addLayer", "Add PostGIS Layer", "PostGIS › Layer", add_layer).when(need_connection).params(
            r#"{"table": "table, schema.table, or table:column for one of several geometry columns", "where"?: "an SQL condition on the table's columns", "limit"?: "the most rows (default and most: 100000)", "name"?: string, "crs"?: "EPSG:… of a geometry that doesn't say"}"#,
        ),
    ]
}

pub(crate) fn need_connection(s: &Session) -> Option<&'static str> {
    s.postgis.is_none().then_some(NOT_CONNECTED)
}

fn status(s: &mut Session, _: &Value) -> CmdResult {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = s;
        Ok(Value::Null)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rustgis_data::source::duckdb::Database;
        Ok(match &s.postgis {
            Some(dsn) => {
                let db = Database::Postgres(dsn.clone());
                json!({"connected": true, "connection": db.label(), "database": db.short()})
            }
            None => json!({"connected": false}),
        })
    }
}

fn disconnect(s: &mut Session, _: &Value) -> CmdResult {
    let was = s.postgis.take();
    Ok(Value::Bool(was.is_some()))
}

/// The connection the parameters give, without its password (which is remembered for the
/// session): a whole connection string, or its parts.
#[cfg(not(target_arch = "wasm32"))]
fn connection_param(p: &Value) -> Result<String, String> {
    use rustgis_data::source::duckdb::{Database, remember_password};
    let text = |k: &str| -> Option<String> {
        match p.get(k)? {
            Value::String(v) => Some(v.trim().to_string()).filter(|v| !v.is_empty()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    };
    let spec = match text("connection") {
        // A bare keyword string is a PostgreSQL one (as Add From Database takes it).
        Some(c) if c.contains('=') && !c.contains("://") && !c.to_ascii_lowercase().starts_with("postgres:") => format!("postgres:{c}"),
        Some(c) => c,
        None => {
            if text("host").is_none() {
                return Err("give the host, or a whole connection string".into());
            }
            if let Some(m) = text("sslmode")
                && !SSL_MODES.contains(&m.to_ascii_lowercase().as_str())
            {
                return Err(format!("sslmode '{}': one of {}", clip(&m), SSL_MODES.join(", ")));
            }
            let quote = |v: &str| format!("'{}'", v.replace('\\', "\\\\").replace('\'', "\\'"));
            let parts: Vec<String> = [("host", "host"), ("port", "port"), ("dbname", "database"), ("user", "user"), ("sslmode", "sslmode")]
                .iter()
                .filter_map(|(key, param)| text(param).map(|v| format!("{key}={}", quote(&v))))
                .collect();
            format!("postgres:{}", parts.join(" "))
        }
    };
    let Database::Postgres(dsn) = Database::parse(&spec) else {
        return Err("not a PostgreSQL connection: host=… dbname=… user=…, or a postgresql:// address".into());
    };
    if let Some(pw) = ps(p, "password").filter(|pw| !pw.is_empty()) {
        remember_password(&dsn, pw);
    }
    Ok(dsn)
}

/// Text from a parameter, shortened for a message.
#[cfg(not(target_arch = "wasm32"))]
fn clip(s: &str) -> String {
    s.chars().take(40).collect()
}

fn connect(s: &mut Session, p: &Value) -> CmdResult {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, p);
        Err("PostGIS databases are read in the desktop app".into())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rustgis_data::source::{duckdb::Database, postgis};
        let dsn = connection_param(p)?;
        let db = Database::Postgres(dsn.clone());
        let label = db.short();
        let conn = postgis::connect(&dsn, true).map_err(|e| format!("{label}: {e}"))?;
        // Listing the tables checks the database has PostGIS.
        let tables = postgis::tables(&conn).map_err(|e| format!("{label}: {e}"))?;
        s.postgis = Some(dsn);
        s.notify(format!("Connected to {label}: {} tables with geometry", tables.len()), false);
        Ok(json!({"connected": true, "connection": db.label(), "database": label, "tables": tables.len()}))
    }
}

fn tables(s: &mut Session, p: &Value) -> CmdResult {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, p);
        Err("PostGIS databases are read in the desktop app".into())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rustgis_data::source::{duckdb::Database, postgis};
        let dsn = s.postgis.clone().ok_or(NOT_CONNECTED)?;
        let label = Database::Postgres(dsn.clone()).short();
        let conn = postgis::connect(&dsn, true).map_err(|e| format!("{label}: {e}"))?;
        let all = postgis::tables(&conn).map_err(|e| format!("{label}: {e}"))?;
        let schema = ps(p, "schema").map(str::trim).filter(|v| !v.is_empty());
        let listed: Vec<Value> = all
            .iter()
            .filter(|t| schema.is_none_or(|v| t.schema.eq_ignore_ascii_case(v)))
            .take(MAX_LISTED)
            .map(|t| {
                json!({"name": t.label(), "schema": t.schema, "table": t.name, "column": t.column, "type": t.kind, "srid": t.srid, "geography": t.geography})
            })
            .collect();
        Ok(json!({"database": label, "tables": listed}))
    }
}

fn add_layer(s: &mut Session, p: &Value) -> CmdResult {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (s, p);
        Err("PostGIS databases are read in the desktop app".into())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rustgis_data::source::{FeatureSource, duckdb::Database, postgis};
        use rustgis_map::source::{PostgisEdit, PostgisOrigin};
        let dsn = s.postgis.clone().ok_or(NOT_CONNECTED)?;
        let name = ps(p, "table").map(str::trim).filter(|t| !t.is_empty()).ok_or("table is required")?;
        let db = Database::Postgres(dsn.clone());
        let label = db.short();
        let err = |e: rustgis_data::DataError| format!("{label}: {e}");
        let crs = ps(p, "crs").map(str::trim).filter(|c| !c.is_empty());
        let condition = ps(p, "where");
        let limit = f(p, "limit").map_or(postgis::MAX_EDITABLE, |n| n.clamp(1.0, postgis::MAX_EDITABLE as f64) as usize);
        let conn = postgis::connect(&dsn, true).map_err(err)?;
        let table = postgis::find_table(&conn, name).map_err(err)?;
        let families = postgis::families(&conn, &table).map_err(err)?;
        if families.is_empty() {
            return Err(format!("{label}: {}: the table has no rows with a geometry", table.label()));
        }
        let mixed = families.len() > 1;
        let empty_before = s.map().layers.is_empty();
        let (mut ids, mut loaded, mut matched, mut writable) = (Vec::new(), 0u64, 0u64, true);
        for family in families {
            let src = postgis::PostgisSource::open(conn.clone(), table.clone(), Some(family), crs).map_err(err)?;
            let key = src.key().map(str::to_string);
            let editable = src.read_editable(condition, limit).map_err(err)?;
            let layer_name = match ps(p, "name").map(str::trim).filter(|n| !n.is_empty() && !mixed) {
                Some(n) => n.to_string(),
                None if mixed => format!("{} ({})", table.label(), family.label()),
                None => table.label(),
            };
            let info = src.info();
            let mut ds = rustgis_data::Dataset::new(layer_name.clone(), info.geometry_type, info.fields.clone());
            ds.crs = u32::try_from(src.srid()).ok().filter(|&c| c != 0 && c != 4326).and_then(rustgis_data::raster::Crs::from_epsg);
            let n = editable.features.len() as u64;
            ds.features = editable.features.into();
            if n < editable.matched {
                s.notify(
                    format!(
                        "{layer_name}: loaded {n} of {} rows; narrow them with a condition (where) to edit the others",
                        editable.matched
                    ),
                    false,
                );
            }
            if !editable.writable {
                s.notify(format!("{layer_name}: this user can't insert, update or delete its rows, so edits can't be saved to it"), false);
            }
            let id = super::layer::add_dataset(s, ds);
            if let Some(l) = s.map_mut().layer_mut(id) {
                l.postgis = Some(PostgisEdit {
                    origin: PostgisOrigin {
                        database: db.label(),
                        table: table.label(),
                        family: Some(family),
                        crs: crs.map(str::to_string),
                    },
                    key: key.unwrap_or_default(),
                    srid: src.srid(),
                    writable: editable.writable,
                    versions: editable.versions.into_iter().collect(),
                });
            }
            ids.push(id);
            loaded += n;
            matched += editable.matched;
            writable &= editable.writable;
        }
        if empty_before {
            let e = ids.iter().filter_map(|id| s.map().layer(*id)).fold(rustgis_geom::Extent::EMPTY, |e, l| e.union(&l.extent()));
            if !e.is_empty() {
                s.zoom_to(&e);
            }
        }
        Ok(json!({"layers": ids, "features": loaded, "matched": matched, "writable": writable}))
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use serde_json::json;

    #[test]
    fn connections_are_built_from_their_parts_without_the_password() {
        let dsn = super::connection_param(&json!({"host": "db.example", "port": 5433, "database": "my gis", "user": "o'neil", "password": "s3cret", "sslmode": "require"}))
            .unwrap_or_else(|e| panic!("{e}"));
        for part in ["host=db.example", "port=5433", "dbname='my gis'", r"user='o\'neil'", "sslmode=require"] {
            assert!(dsn.contains(part), "{part} in {dsn}");
        }
        assert!(!dsn.contains("s3cret"), "{dsn}");
        // A whole connection string wins over the parts, and loses its password.
        let dsn = super::connection_param(&json!({"connection": "host=h dbname=g password=pw", "host": "other"}))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(dsn, "host=h dbname=g");
        let dsn = super::connection_param(&json!({"connection": "postgresql://u:pw@h:5432/g"})).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(dsn, "postgresql://u@h:5432/g");
        // What isn't a PostgreSQL connection is refused before anything is sent.
        for bad in [json!({}), json!({"host": " "}), json!({"host": "h", "sslmode": "bogus"}), json!({"connection": "mysql://u@h/db"})] {
            assert!(super::connection_param(&bad).is_err(), "{bad}");
        }
    }
}
