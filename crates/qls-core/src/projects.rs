//! The project list, shared by every device that uses this Kokon (the desktop app, a phone's
//! browser). Before 0.11 each app kept it in its own browser storage; each one now sends that
//! copy once (`import`) and from then on reads and saves here.
//!
//! A project is the UI's: a title and its versions in order (`items`), each one a gallery image,
//! an imported image or a job still running. Two devices can change the same project at once
//! (an edit made on the phone while the computer has it open), so a save is merged, not
//! overwritten: the saving device sends the versions it knew (`known`), and versions it did not
//! know (added elsewhere since) are kept, while versions it knew and left out were removed by it.
//! Removed projects leave a tombstone, so a device that had not heard yet cannot bring them back.
//!
//! Storage (task-032): the list lives in the box's database (`gallery.db`, beside the gallery's
//! own records: the `projects` and `project_items` tables) — the old `projects.json` was kept
//! as a file because it was the product's user-visible format and the stock path had to stay
//! byte-identical to upstream's; that constraint retired with the stock path (task-031), and
//! with it the reason for the parallel `owner` side-map in the file (D-MU13) — the `owner`
//! column is first-class now. The database is the source of truth from the first import,
//! which runs once (a table that already holds rows is never re-imported); `projects.json` is
//! still written on every change for this one schema version (18) as a read-only export in
//! the exact old shape — a rollback to the previous image reads it, and the pre-import bytes
//! are snapshotted to `projects.json.pre-db` — and the next schema bump retires it (D-032-1).
//!
//! Multi-user (D-MU13): the `owner` column records which named account each project belongs to
//! (NULL = the machine's shared list, the pre-multi-user one). The service decides it from
//! the caller — it never travels in a `Project` and a client cannot set it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::{AppError, AppResult, ErrorCode};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub key: String,
    /// "gen", "input" or "job".
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gen_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    /// Fields of newer apps, kept as they are (the request of a running job among them).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub id: String,
    pub title: String,
    /// The title was typed by the user (otherwise it follows the first prompt).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub named: Option<bool>,
    /// Milliseconds since 1970: the list is sorted by it.
    pub updated_at: f64,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub meta: Meta,
    #[serde(default)]
    pub items: Vec<Item>,
    #[serde(default)]
    pub selected: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loras: Option<Vec<String>>,
    /// Bumped by every change here; a device ignores news of a revision it already has.
    #[serde(default)]
    pub rev: u64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// The file the list used to be (D-MU13): kept only as the read-only export of the one
/// transition version — the exact old serialisation, so a previous image reads it as it
/// always has (the `owner` key skipped while empty).
#[derive(Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    projects: HashMap<String, Project>,
    /// Removed project id → when (ms).
    #[serde(default)]
    deleted: HashMap<String, f64>,
    /// Project id → the named account that owns it (D-MU13). Absent = the machine's shared
    /// project (the pre-multi-user list). Skipped while empty: a single-user file — where
    /// no named account can exist — never gains the key.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    owner: HashMap<String, String>,
}

/// The gallery and input records of a project's versions, sent in one answer to a browser
/// opening it (one request per version is slow on a phone). A version whose record is gone
/// (deleted from the gallery) is left out.
#[derive(Debug, Default, Serialize)]
pub struct Records {
    pub generations: HashMap<String, crate::store::GenerationRecord>,
    pub inputs: HashMap<String, crate::store::InputRecord>,
}

/// What a change did, for the event the other devices hear.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub id: String,
    pub rev: u64,
    pub deleted: bool,
    /// The owner name (D-MU13): the event's filter reaches only that account's devices.
    /// None = the shared project (the machine and the operator hear it).
    pub user: Option<String>,
}

pub struct ProjectStore {
    db: Mutex<Connection>,
    /// The one-version read-only export of D-032-1 (off for in-memory stores).
    export_path: Option<PathBuf>,
}

/// The project's row plus its items, in list order.
struct Stored {
    project: Project,
    /// NULL = the machine's shared project.
    owner: Option<String>,
    /// NOT NULL = a tombstone (removed; the id cannot come back).
    deleted_at: Option<f64>,
}

fn db_err(e: rusqlite::Error) -> AppError {
    AppError::new(ErrorCode::Filesystem, &format!("the projects store (the box's database): {e}"))
}

fn source(i: &Item) -> Option<String> {
    i.gen_id
        .as_ref()
        .map(|g| format!("gen:{g}"))
        .or_else(|| i.input_id.as_ref().map(|r| format!("input:{r}")))
}

/// The images the project's layers lab made (its `made` list, see src/lib/layerDoc.ts): steps
/// such as one object's crop, never the project's cover.
fn lab_made(p: &Project) -> HashSet<&str> {
    p.extra
        .get("lab")
        .and_then(|l| l.get("made"))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// The cover (the latest image that is not a lab step) and the number of versions, as the UI
/// computes them.
fn refresh_meta(p: &mut Project) {
    let lab = lab_made(p);
    p.meta.cover = p
        .items
        .iter()
        .rev()
        .filter_map(source)
        .find(|s| !lab.contains(s.as_str()));
    p.meta.count = p.items.iter().filter(|i| i.kind != "job").count() as u32;
}

/// Merges a device's save into the stored copy (see the module comment). Pure, for the tests.
pub fn merge(stored: Option<&Project>, incoming: Project, known: &HashSet<String>) -> Project {
    let Some(stored) = stored else {
        let mut p = incoming;
        p.rev = 1;
        refresh_meta(&mut p);
        return p;
    };
    let mut items = incoming.items.clone();
    // Versions added elsewhere since this device last heard: each goes back after the version
    // it followed there (or at the end).
    let mut anchor: Option<String> = None;
    for it in &stored.items {
        if items.iter().any(|x| x.key == it.key) {
            anchor = Some(it.key.clone());
            continue;
        }
        if known.contains(&it.key) {
            continue;
        }
        let at = anchor
            .as_ref()
            .and_then(|a| items.iter().position(|x| &x.key == a))
            .map(|k| k + 1)
            .unwrap_or(items.len());
        items.insert(at, it.clone());
        anchor = Some(it.key.clone());
    }
    // A running job whose images arrived (here or on another device) is no longer pending.
    let finished: HashSet<String> = items
        .iter()
        .filter(|i| i.kind != "job")
        .filter_map(|i| i.job_id.clone())
        .collect();
    items.retain(|i| i.kind != "job" || !i.job_id.as_ref().is_some_and(|j| finished.contains(j)));

    let mut meta = incoming.meta.clone();
    // A name typed on another device is kept over a title this device made up from a prompt.
    if stored.meta.named == Some(true) && incoming.meta.named != Some(true) {
        meta.title = stored.meta.title.clone();
        meta.named = Some(true);
    }
    meta.updated_at = meta.updated_at.max(stored.meta.updated_at);
    let has = |k: &Option<String>| {
        k.as_ref()
            .is_some_and(|k| items.iter().any(|i| &i.key == k))
    };
    let selected = if has(&incoming.selected) {
        incoming.selected.clone()
    } else if has(&stored.selected) {
        stored.selected.clone()
    } else {
        items.last().map(|i| i.key.clone())
    };
    // Fields a saving device did not send (one that knows nothing of the layers lab) are kept.
    let mut extra = stored.extra.clone();
    extra.extend(incoming.extra);
    let mut p = Project {
        meta,
        items,
        selected,
        loras: incoming.loras.clone(),
        rev: stored.rev + 1,
        extra,
    };
    refresh_meta(&mut p);
    p
}

/// The tables (D-032-2): the list in the box's database. A tombstone is the row itself with
/// `deleted_at` set — the row is never physically deleted, so a removed id cannot be reused.
fn create_tables(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS projects (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            named INTEGER,
            updated_at REAL NOT NULL,
            cover TEXT,
            count INTEGER NOT NULL DEFAULT 0,
            selected TEXT,
            loras TEXT,
            rev INTEGER NOT NULL,
            owner TEXT,
            deleted_at REAL,
            extra TEXT NOT NULL DEFAULT '{}'
        );
        CREATE TABLE IF NOT EXISTS project_items (
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            key TEXT NOT NULL,
            kind TEXT NOT NULL,
            gen_id TEXT,
            input_id TEXT,
            job_id TEXT,
            position INTEGER NOT NULL,
            extra TEXT NOT NULL DEFAULT '{}',
            PRIMARY KEY (project_id, key)
        );
        CREATE INDEX IF NOT EXISTS items_pos ON project_items(project_id, position);",
    )
    .map_err(db_err)?;
    // The database's schema generation (the store's own tables are 2; the projects
    // tables moved in at 3). Never downgraded: a previous image sets 2 on its opens,
    // and it is the content, not the number, that it reads.
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(db_err)?;
    if version < 3 {
        conn.pragma_update(None, "user_version", 3).map_err(db_err)?;
    }
    Ok(())
}

fn conn_pragmas(conn: &Connection) -> AppResult<()> {
    conn.pragma_update(None, "foreign_keys", 1).map_err(db_err)
}

/// The stored row plus its items; a tombstone is the row with `deleted_at` set (no items).
fn read_stored(conn: &Connection, id: &str) -> AppResult<Option<Stored>> {
    let mut st = conn
        .prepare("SELECT title, named, updated_at, cover, count, selected, loras, rev, owner, deleted_at, extra FROM projects WHERE id = ?1")
        .map_err(db_err)?;
    let row = st
        .query_map(params![id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<bool>>(1)?,
                r.get::<_, f64>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, u32>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, Option<String>>(8)?,
                r.get::<_, Option<f64>>(9)?,
                r.get::<_, String>(10)?,
            ))
        })
        .map_err(db_err)?
        .next()
        .transpose()
        .map_err(db_err)?;
    let Some((title, named, updated_at, cover, count, selected, loras, rev, owner, deleted_at, extra)) =
        row
    else {
        return Ok(None);
    };
    let rev = rev as u64;
    let mut items = Vec::new();
    let mut is = conn
        .prepare("SELECT key, kind, gen_id, input_id, job_id, extra FROM project_items WHERE project_id = ?1 ORDER BY position")
        .map_err(db_err)?;
    for r in is.query_map(params![id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, String>(5)?,
        ))
    })
    .map_err(db_err)?
    {
        let (key, kind, gen_id, input_id, job_id, extra) = r.map_err(db_err)?;
        items.push(Item {
            key,
            kind,
            gen_id,
            input_id,
            job_id,
            extra: serde_json::from_str(&extra).unwrap_or_default(),
        });
    }
    let project = Project {
        meta: Meta {
            id: id.to_string(),
            title,
            named,
            updated_at,
            cover,
            count,
        },
        items,
        selected,
        loras: loras.as_deref().and_then(|s| serde_json::from_str(s).ok()),
        rev,
        extra: serde_json::from_str(&extra).unwrap_or_default(),
    };
    Ok(Some(Stored {
        project,
        owner,
        deleted_at,
    }))
}

/// The project's rows: the row plus its items (in order), replacing what was there.
fn write_stored(
    conn: &rusqlite::Transaction,
    p: &Project,
    owner: Option<&str>,
    deleted_at: Option<f64>,
) -> AppResult<()> {
    let loras = p
        .loras
        .as_ref()
        .map(|l| serde_json::to_string(l))
        .transpose()
        .map_err(AppError::internal)?;
    conn.execute(
        "INSERT INTO projects (id, title, named, updated_at, cover, count, selected, loras, rev, owner, deleted_at, extra)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT (id) DO UPDATE SET
           title = excluded.title,
           named = excluded.named,
           updated_at = excluded.updated_at,
           cover = excluded.cover,
           count = excluded.count,
           selected = excluded.selected,
           loras = excluded.loras,
           rev = excluded.rev,
           owner = excluded.owner,
           deleted_at = excluded.deleted_at,
           extra = excluded.extra",
        params![
            p.meta.id,
            p.meta.title,
            p.meta.named,
            p.meta.updated_at,
            p.meta.cover,
            p.meta.count,
            p.selected,
            loras,
            p.rev as i64,
            owner,
            deleted_at,
            serde_json::to_string(&p.extra).unwrap_or_else(|_| "{}".into())
        ],
    )
    .map_err(db_err)?;
    conn.execute("DELETE FROM project_items WHERE project_id = ?1", params![p.meta.id])
        .map_err(db_err)?;
    for (i, it) in p.items.iter().enumerate() {
        conn.execute(
            "INSERT OR REPLACE INTO project_items (project_id, key, kind, gen_id, input_id, job_id, position, extra)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                p.meta.id,
                it.key,
                it.kind,
                it.gen_id,
                it.input_id,
                it.job_id,
                i as i64,
                serde_json::to_string(&it.extra).unwrap_or_else(|_| "{}".into())
            ],
        )
        .map_err(db_err)?;
    }
    Ok(())
}

impl ProjectStore {
    /// Opens the box's database and makes sure the tables exist. If the list is still in
    /// `projects.json` (a box that has never run this schema), it is imported into the
    /// tables once — idempotent, keyed on the tables being empty — and snapshotted to
    /// `projects.json.pre-db` first (D-032-1). `export_file` turns on the one-version
    /// read-only export of the file (the engine sets it from the config's schema version).
    pub fn open(data_dir: &Path, export_file: bool) -> AppResult<Self> {
        let path = data_dir.join("gallery.db");
        let conn = Connection::open(&path).map_err(db_err)?;
        conn_pragmas(&conn)?;
        create_tables(&conn)?;
        let file_path = data_dir.join("projects.json");
        let store = Self {
            db: Mutex::new(conn),
            export_path: if export_file { Some(file_path.clone()) } else { None },
        };
        store.import_from_file(&file_path)?;
        Ok(store)
    }

    /// Not persisted (tests).
    pub fn in_memory() -> Self {
        let conn = Connection::open_in_memory().expect("in-memory database");
        conn_pragmas(&conn).expect("pragmas");
        create_tables(&conn).expect("tables");
        Self {
            db: Mutex::new(conn),
            export_path: None,
        }
    }

    /// The one-time import of `projects.json` into the tables (D-032-1): runs only when
    /// the tables hold no rows at all — a box that already migrated never re-imports.
    /// A file that cannot be read is kept aside as `.bad`, as the file store did.
    fn import_from_file(&self, file_path: &Path) -> AppResult<()> {
        let text = match std::fs::read_to_string(file_path) {
            Ok(t) => t,
            Err(_) => return Ok(()), // no file: nothing to import (a fresh box).
        };
        // The tables already hold the world (an earlier start imported it): the file is a
        // stale export now and is left alone.
        let mut db = self.db.lock().unwrap();
        let count: i64 = db
            .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get::<_, i64>(0))
            .map_err(db_err)?;
        if count > 0 {
            return Ok(());
        }
        let file: File = match serde_json::from_str(&text) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("projects.json is unreadable ({e}); kept as projects.json.bad");
                let _ = std::fs::rename(file_path, file_path.with_extension("json.bad"));
                return Ok(());
            }
        };
        if file.projects.is_empty() && file.deleted.is_empty() {
            return Ok(()); // an empty file imports nothing; the tables stay empty.
        }
        // The one-step rollback: the bytes as they were before the import.
        if self.export_path.is_some() {
            let _ = std::fs::copy(file_path, file_path.with_extension("json.pre-db"));
        }
        let tx = db.transaction().map_err(db_err)?;
        for (id, p) in &file.projects {
            write_stored(
                &tx,
                p,
                file.owner.get(id).map(String::as_str),
                None,
            )?;
        }
        for (id, when) in &file.deleted {
            let owner = file.owner.get(id).map(String::as_str);
            tx.execute(
                "INSERT INTO projects (id, title, updated_at, rev, owner, deleted_at)
                 VALUES (?1, '', 0, 0, ?2, ?3)
                 ON CONFLICT (id) DO UPDATE SET deleted_at = excluded.deleted_at",
                params![id, owner, when],
            )
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        tracing::info!(
            "projects: imported {} projects and {} tombstones from projects.json into the database",
            file.projects.len(),
            file.deleted.len()
        );
        drop(db);
        if self.export_path.is_some() {
            self.write_export();
        }
        Ok(())
    }

    /// The read-only file export of the one transition version (D-032-1): the exact old
    /// shape, written after every change.
    fn write_export(&self) {
        let Some(path) = &self.export_path else {
            return;
        };
        let db = self.db.lock().unwrap();
        // The export is a convenience of the transition; a store error here is reported
        // where the change happened, not in the export's shadow.
        let Ok(projects) = self.export_lists(&db) else {
            return;
        };
        let Ok(deleted) = self.export_deleted(&db) else {
            return;
        };
        let Ok(owner) = self.export_owner(&db) else {
            return;
        };
        drop(db);
        let file = File {
            projects,
            deleted,
            owner,
        };
        let Ok(text) = serde_json::to_vec(&file) else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("json.part");
        let _ = std::fs::write(&tmp, text);
        let _ = std::fs::rename(&tmp, path);
    }

    fn export_lists(&self, db: &Connection) -> AppResult<HashMap<String, Project>> {
        let mut out = HashMap::new();
        let mut st = db.prepare("SELECT id FROM projects WHERE deleted_at IS NULL ORDER BY id").map_err(db_err)?;
        let ids: Vec<String> = st
            .query_map([], |r| r.get(0))
            .map_err(db_err)?
            .collect::<Result<_, _>>()
            .map_err(db_err)?;
        for id in ids {
            if let Some(s) = read_stored(db, &id)? {
                out.insert(id, s.project);
            }
        }
        Ok(out)
    }

    fn export_deleted(&self, db: &Connection) -> AppResult<HashMap<String, f64>> {
        let mut out = HashMap::new();
        let mut st = db.prepare("SELECT id, deleted_at FROM projects WHERE deleted_at IS NOT NULL").map_err(db_err)?;
        let rows = st
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))
            .map_err(db_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)?;
        for (id, when) in rows {
            out.insert(id, when);
        }
        Ok(out)
    }

    fn export_owner(&self, db: &Connection) -> AppResult<HashMap<String, String>> {
        let mut out = HashMap::new();
        let mut st = db
            .prepare("SELECT id, owner FROM projects WHERE deleted_at IS NULL AND owner IS NOT NULL")
            .map_err(db_err)?;
        let rows = st
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(db_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)?;
        for (id, o) in rows {
            out.insert(id, o);
        }
        Ok(out)
    }

    /// Every project, most recent first — the machine's view (the token, the operator).
    pub fn list(&self) -> Vec<Project> {
        self.list_for(None)
    }

    /// Every project this caller sees, most recent first (D-MU13): a named account only
    /// its own; the token and the operator everything.
    pub fn list_for(&self, user: Option<&str>) -> Vec<Project> {
        let db = self.db.lock().unwrap();
        let mut v: Vec<Project> = Vec::new();
        let mut st = match db.prepare("SELECT id FROM projects WHERE deleted_at IS NULL") {
            Ok(s) => s,
            Err(_) => return v,
        };
        let ids = match st
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(db_err)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(ids) => ids,
            Err(_) => return v,
        };
        for id in ids {
            let Some(s) = read_stored(&db, &id).unwrap_or(None) else {
                continue;
            };
            let mine = match user {
                Some(u) => s.owner.as_deref() == Some(u),
                None => true,
            };
            if mine {
                v.push(s.project);
            }
        }
        v.sort_by(|a, b| {
            b.meta
                .updated_at
                .total_cmp(&a.meta.updated_at)
                .then_with(|| a.meta.id.cmp(&b.meta.id))
        });
        v
    }

    /// The project, live or a tombstone (the caller decides what a tombstone means).
    pub fn get(&self, id: &str) -> Option<Project> {
        let db = self.db.lock().unwrap();
        read_stored(&db, id)
            .ok()
            .flatten()
            .filter(|s| s.deleted_at.is_none())
            .map(|s| s.project)
    }

    /// The owner name (D-MU13); None = the machine's shared project (or a removed one —
    /// the owner travels with the removal, as the file's side-map did).
    pub fn owner_of(&self, id: &str) -> Option<String> {
        let db = self.db.lock().unwrap();
        read_stored(&db, id)
            .ok()
            .flatten()
            .filter(|s| s.deleted_at.is_none())
            .and_then(|s| s.owner)
    }

    /// The project, if this caller may see it (D-MU13): a named account only its own —
    /// shared, another's and absent are all the same absence to it (no existence leak);
    /// the token and the operator everything. A tombstone is an absence.
    pub fn get_for(&self, id: &str, user: Option<&str>) -> Option<Project> {
        let db = self.db.lock().unwrap();
        let s = read_stored(&db, id).ok().flatten()?;
        if s.deleted_at.is_some() {
            return None;
        }
        if let Some(u) = user
            && s.owner.as_deref() != Some(u)
        {
            return None;
        }
        Some(s.project)
    }

    /// Saves a device's copy of project `id`, merged with what is stored, as this caller
    /// (D-MU13): a named account writes only its own (shared and another's are absent to
    /// it — the stock 404); the token and the operator write everything, and a merge keeps
    /// the project's owner. A new project is owned by the caller (shared for the machine
    /// and the operator). A removed project stays removed (NotFound). One transaction:
    /// read the stored values, run the pure merge, write the rows back.
    pub fn put(
        &self,
        id: &str,
        mut incoming: Project,
        known: &HashSet<String>,
        user: Option<&str>,
    ) -> AppResult<(Project, Change)> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction().map_err(db_err)?;
        let stored = read_stored(&tx, id)?;
        if let Some(s) = &stored
            && s.deleted_at.is_some()
        {
            return Err(AppError::new(
                ErrorCode::NotFound,
                "This project was removed on another device.",
            ));
        }
        if let Some(u) = user
            && stored.is_some()
            && stored.as_ref().unwrap().owner.as_deref() != Some(u)
        {
            return Err(AppError::new(ErrorCode::NotFound, "No such project."));
        }
        let keep_owner = stored.as_ref().and_then(|s| s.owner.clone());
        incoming.meta.id = id.to_string();
        let merged = merge(stored.as_ref().map(|s| &s.project), incoming, known);
        let owner = if stored.is_none() {
            user.map(str::to_string).or(keep_owner)
        } else {
            keep_owner
        };
        write_stored(&tx, &merged, owner.as_deref(), None)?;
        tx.commit().map_err(db_err)?;
        drop(db);
        self.write_export();
        let change = Change {
            id: id.to_string(),
            rev: merged.rev,
            deleted: false,
            user: owner,
        };
        Ok((merged, change))
    }

    /// Removes the project (as this caller — a named account only its own, D-MU13) and
    /// leaves a tombstone, so a device that had not heard yet cannot bring it back.
    /// Removing an absent id leaves (or refreshes) a tombstone, as the file store did.
    pub fn delete(&self, id: &str, now_ms: f64, user: Option<&str>) -> AppResult<Change> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction().map_err(db_err)?;
        let stored = read_stored(&tx, id)?;
        let owner = stored.as_ref().and_then(|s| s.owner.clone());
        if let Some(u) = user
            && stored.as_ref().and_then(|s| s.owner.as_deref()) != Some(u)
        {
            return Err(AppError::new(ErrorCode::NotFound, "No such project."));
        }
        let title = stored.as_ref().map(|s| s.project.meta.title.clone()).unwrap_or_default();
        let was_live = stored.as_ref().is_some_and(|s| s.deleted_at.is_none());
        if was_live {
            tx.execute("DELETE FROM project_items WHERE project_id = ?1", params![id])
                .map_err(db_err)?;
            tx.execute(
                "UPDATE projects SET deleted_at = ?2, owner = NULL, title = ?3 WHERE id = ?1",
                params![id, now_ms, title],
            )
            .map_err(db_err)?;
        } else {
            // A tombstone (or a never-seen id): keep it; a never-seen id becomes one.
            tx.execute(
                "INSERT INTO projects (id, title, updated_at, rev, deleted_at)
                 VALUES (?1, ?2, 0, 0, ?3)
                 ON CONFLICT (id) DO UPDATE SET deleted_at = excluded.deleted_at",
                params![id, title, now_ms],
            )
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        drop(db);
        self.write_export();
        // The event's owner: a live removal carries the owner (the file's side-map entry
        // went with the project); a tombstone refresh does not.
        Ok(Change {
            id: id.to_string(),
            rev: 0,
            deleted: true,
            user: if was_live { owner } else { None },
        })
    }

    /// A device's list from before 0.11 (its browser storage), merged in once — as this
    /// caller (D-MU13): a named account merges into and creates only its own; projects it
    /// may not see (the shared list, another's) are skipped, untouched. The machine and the
    /// operator merge into the shared ones and create shared.
    /// Sending the same list again changes nothing.
    pub fn import(&self, list: Vec<Project>, user: Option<&str>) -> AppResult<Vec<Change>> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction().map_err(db_err)?;
        let mut changes = Vec::new();
        for p in list {
            let id = p.meta.id.clone();
            if id.is_empty() {
                continue;
            }
            let stored = read_stored(&tx, &id)?;
            if stored.as_ref().is_some_and(|s| s.deleted_at.is_some()) {
                continue; // a removed id is not brought back by an import.
            }
            // D-MU13: the import is namespaced by the caller. An existing project is
            // merged only if the caller may see it — its own; for the machine and the
            // operator, everything (the shared list is not a named account's). A new id
            // is created under the caller.
            let visible = match (user, stored.as_ref().is_some(), stored.as_ref().and_then(|s| s.owner.as_deref())) {
                (Some(u), true, Some(o)) => o == u,
                (Some(_), true, None) => false,
                (None, _, _) => true,
                (Some(_), false, _) => true,
            };
            if !visible {
                continue;
            }
            let merged = merge(stored.as_ref().map(|s| &s.project), p, &HashSet::new());
            let is_new = stored.is_none();
            if !is_new
                && let Some(s) = &stored
                && merged.items == s.project.items && merged.meta.title == s.project.meta.title
            {
                continue; // the same list again changes nothing.
            }
            let owner = if is_new {
                user.map(str::to_string)
            } else {
                stored.as_ref().and_then(|s| s.owner.clone())
            };
            write_stored(&tx, &merged, owner.as_deref(), None)?;
            changes.push(Change {
                id,
                rev: merged.rev,
                deleted: false,
                user: owner,
            });
        }
        if !changes.is_empty() {
            tx.commit().map_err(db_err)?;
        } else {
            tx.rollback().map_err(db_err)?;
        }
        drop(db);
        if !changes.is_empty() {
            self.write_export();
        }
        Ok(changes)
    }

    /// All projects owned by a deleted account (D-MU13): they reference its rows, and the
    /// machine's list would otherwise grow a ghost owner. Returns the changes to emit.
    pub fn delete_user_projects(&self, user: &str) -> AppResult<Vec<Change>> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction().map_err(db_err)?;
        let now = crate::engine::now_ms();
        let ids = {
            let mut st = tx
                .prepare("SELECT id FROM projects WHERE deleted_at IS NULL AND owner = ?1")
                .map_err(db_err)?;
            st.query_map(params![user], |r| r.get::<_, String>(0))
                .map_err(db_err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(db_err)?
        };
        let mut changes = Vec::new();
        for id in ids {
            let Some(s) = read_stored(&tx, &id)? else {
                continue;
            };
            tx.execute("DELETE FROM project_items WHERE project_id = ?1", params![id])
                .map_err(db_err)?;
            tx.execute(
                "UPDATE projects SET deleted_at = ?2, owner = NULL WHERE id = ?1",
                params![id, now],
            )
            .map_err(db_err)?;
            changes.push(Change {
                id,
                rev: s.project.rev,
                deleted: true,
                user: Some(user.to_string()),
            });
        }
        if !changes.is_empty() {
            tx.commit().map_err(db_err)?;
        } else {
            tx.rollback().map_err(db_err)?;
        }
        drop(db);
        if !changes.is_empty() {
            self.write_export();
        }
        Ok(changes)
    }

    /// Job `job_id` finished: in every project waiting for it, its images replace the pending
    /// version (keys "r-<image id>", the same the apps use, so a device doing it too adds nothing).
    pub fn job_done(
        &self,
        job_id: &str,
        gen_ids: &[String],
        now_ms: f64,
    ) -> AppResult<Vec<Change>> {
        let mut db = self.db.lock().unwrap();
        let tx = db.transaction().map_err(db_err)?;
        // The live projects, with their owners (the event's filter).
        let live = {
            let mut st = tx
                .prepare("SELECT id, owner FROM projects WHERE deleted_at IS NULL")
                .map_err(db_err)?;
            st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))
                .map_err(db_err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(db_err)?
        };
        let mut changes = Vec::new();
        for (id, owner) in live {
            let Some(s) = read_stored(&tx, &id)? else {
                continue;
            };
            let Some(k) = s
                .project
                .items
                .iter()
                .position(|i| i.kind == "job" && i.job_id.as_deref() == Some(job_id))
            else {
                continue;
            };
            let mut p = s.project;
            let results: Vec<Item> = gen_ids
                .iter()
                .map(|g| Item {
                    key: format!("r-{g}"),
                    kind: "gen".into(),
                    gen_id: Some(g.clone()),
                    input_id: None,
                    job_id: Some(job_id.to_string()),
                    extra: Map::new(),
                })
                .filter(|r| !p.items.iter().any(|i| i.key == r.key))
                .collect();
            let pending = p.items[k].key.clone();
            let first = results.first().map(|r| r.key.clone());
            p.items.splice(k..k + 1, results);
            if p.selected.as_deref() == Some(pending.as_str()) {
                p.selected = first.or_else(|| p.items.last().map(|i| i.key.clone()));
            }
            p.meta.updated_at = p.meta.updated_at.max(now_ms);
            p.rev += 1;
            refresh_meta(&mut p);
            write_stored(&tx, &p, owner.as_deref(), None)?;
            changes.push(Change {
                id: p.meta.id.clone(),
                rev: p.rev,
                deleted: false,
                user: owner,
            });
        }
        if !changes.is_empty() {
            tx.commit().map_err(db_err)?;
        } else {
            tx.rollback().map_err(db_err)?;
        }
        drop(db);
        if !changes.is_empty() {
            self.write_export();
        }
        Ok(changes)
    }
}

#[cfg(test)]
mod tests {

use super::*;

    fn img(key: &str) -> Item {
        Item {
            key: key.into(),
            kind: "gen".into(),
            gen_id: Some(format!("g{key}")),
            input_id: None,
            job_id: None,
            extra: Map::new(),
        }
    }

    fn job(key: &str, job: &str) -> Item {
        Item {
            key: key.into(),
            kind: "job".into(),
            gen_id: None,
            input_id: None,
            job_id: Some(job.into()),
            extra: Map::new(),
        }
    }

    fn project(id: &str, items: Vec<Item>) -> Project {
        Project {
            meta: Meta {
                id: id.into(),
                title: "A prompt".into(),
                named: None,
                updated_at: 1.0,
                cover: None,
                count: 0,
            },
            selected: items.last().map(|i| i.key.clone()),
            items,
            loras: None,
            rev: 0,
            extra: Map::new(),
        }
    }

    fn keys(p: &Project) -> Vec<&str> {
        p.items.iter().map(|i| i.key.as_str()).collect()
    }

    fn known(k: &[&str]) -> HashSet<String> {
        k.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_cover_is_never_a_layers_lab_step() {
        let mut p = project("p", vec![img("a"), img("b")]);
        p.extra.insert("lab".into(), serde_json::json!({ "v": 2, "docs": [], "made": ["gen:gb"] }));
        let (p, _) = ProjectStore::in_memory()
            .put("p", p, &known(&[]), None)
            .unwrap();
        assert_eq!(p.meta.cover.as_deref(), Some("gen:ga"));
        assert_eq!(p.meta.count, 2);
    }

    #[test]
    fn a_new_project_is_stored_with_its_cover_and_count() {
        let s = ProjectStore::in_memory();
        let (p, c) = s
            .put(
                "p",
                project("p", vec![img("a"), job("b", "j1")]),
                &known(&[]),
                None,
            )
            .unwrap();
        assert_eq!(
            c,
            Change {
                id: "p".into(),
                rev: 1,
                deleted: false,
                user: None
            }
        );
        assert_eq!(p.meta.cover.as_deref(), Some("gen:ga"));
        assert_eq!(p.meta.count, 1);
    }

    #[test]
    fn versions_added_on_two_devices_are_both_kept() {
        let s = ProjectStore::in_memory();
        s.put("p", project("p", vec![img("a")]), &known(&[]), None)
            .unwrap();
        // The phone adds b, the computer (which only knew a) adds c.
        s.put("p", project("p", vec![img("a"), img("b")]), &known(&["a"]), None)
            .unwrap();
        let (p, _) = s
            .put(
                "p",
                project("p", vec![img("a"), img("c")]),
                &known(&["a"]),
                None,
            )
            .unwrap();
        assert_eq!(keys(&p), ["a", "b", "c"]);
        assert_eq!(p.rev, 3);
    }

    #[test]
    fn a_version_removed_by_a_device_that_knew_it_stays_removed() {
        let s = ProjectStore::in_memory();
        s.put("p", project("p", vec![img("a"), img("b")]), &known(&[]), None)
            .unwrap();
        let (p, _) = s
            .put(
                "p",
                project("p", vec![img("a")]),
                &known(&["a", "b"]),
                None,
            )
            .unwrap();
        assert_eq!(keys(&p), ["a"]);
    }

    #[test]
    fn a_version_from_elsewhere_goes_back_after_the_one_it_followed() {
        let stored = project("p", vec![img("a"), img("x"), img("b")]);
        let p = merge(
            Some(&stored),
            project("p", vec![img("a"), img("b"), img("c")]),
            &known(&["a", "b"]),
        );
        assert_eq!(keys(&p), ["a", "x", "b", "c"]);
    }

    #[test]
    fn a_finished_job_replaces_its_pending_version_once() {
        let s = ProjectStore::in_memory();
        s.put(
            "p",
            project("p", vec![img("a"), job("k", "j1")]),
            &known(&[]),
            None,
        )
        .unwrap();
        let ch = s.job_done("j1", &["g1".into(), "g2".into()], 5.0).unwrap();
        assert_eq!(ch.len(), 1);
        let p = s.get("p").unwrap();
        assert_eq!(keys(&p), ["a", "r-g1", "r-g2"]);
        assert_eq!(p.selected.as_deref(), Some("r-g1"));
        assert_eq!(p.meta.count, 3);
        // The app that made it does the same with the same keys: no copies, no pending left.
        let (p, _) = s
            .put(
                "p",
                project(
                    "p",
                    vec![
                        img("a"),
                        job("k", "j1"),
                        Item {
                            key: "r-g1".into(),
                            job_id: Some("j1".into()),
                            ..img("r-g1")
                        },
                    ],
                ),
                &known(&["a", "k"]),
                None,
            )
            .unwrap();
        assert_eq!(keys(&p), ["a", "r-g1", "r-g2"]);
        assert!(s.job_done("j1", &["g1".into()], 6.0).unwrap().is_empty());
    }

    #[test]
    fn a_name_typed_elsewhere_beats_a_title_from_a_prompt() {
        let mut stored = project("p", vec![img("a")]);
        stored.meta.title = "Holiday".into();
        stored.meta.named = Some(true);
        let p = merge(Some(&stored), project("p", vec![img("a")]), &known(&["a"]));
        assert_eq!(p.meta.title, "Holiday");
        let mut renamed = project("p", vec![img("a")]);
        renamed.meta.title = "Trip".into();
        renamed.meta.named = Some(true);
        assert_eq!(
            merge(Some(&stored), renamed, &known(&["a"])).meta.title,
            "Trip"
        );
    }

    #[test]
    fn removed_projects_stay_removed() {
        let s = ProjectStore::in_memory();
        s.put("p", project("p", vec![img("a")]), &known(&[]), None)
            .unwrap();
        s.delete("p", 2.0, None).unwrap();
        assert!(
            s.put("p", project("p", vec![img("a")]), &known(&[]), None)
                .is_err()
        );
        assert!(
            s.import(vec![project("p", vec![img("a")])], None)
                .unwrap()
                .is_empty()
        );
        assert!(s.list().is_empty());
    }

    #[test]
    fn importing_merges_lists_and_twice_changes_nothing() {
        let s = ProjectStore::in_memory();
        s.put("p", project("p", vec![img("a")]), &known(&[]), None)
            .unwrap();
        let phone = vec![
            project("p", vec![img("a"), img("b")]),
            project("q", vec![img("c")]),
        ];
        assert_eq!(s.import(phone.clone(), None).unwrap().len(), 2);
        assert_eq!(keys(&s.get("p").unwrap()), ["a", "b"]);
        assert!(s.import(phone, None).unwrap().is_empty());
        assert_eq!(s.list().len(), 2);
    }

    #[test]
    fn the_list_survives_a_restart_and_keeps_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let s = ProjectStore::open(dir.path(), false).unwrap();
        let mut p = project("p", vec![img("a")]);
        p.items[0].extra.insert("future".into(), Value::from(1));
        s.put("p", p, &known(&[]), None).unwrap();
        let again = ProjectStore::open(dir.path(), false).unwrap();
        assert_eq!(
            again.get("p").unwrap().items[0].extra.get("future"),
            Some(&Value::from(1))
        );
    }

    #[test]
    fn a_save_without_a_field_keeps_the_stored_one() {
        // The layers lab travels with the project; a device that does not send it keeps it.
        let mut stored = project("p", vec![img("a")]);
        stored.extra.insert("lab".into(), Value::from("doc"));
        let stored = merge(None, stored, &known(&[]));
        let m = merge(Some(&stored), project("p", vec![img("a"), img("b")]), &known(&["a"]));
        assert_eq!(m.extra.get("lab"), Some(&Value::from("doc")));
        let mut cleared = project("p", vec![img("a")]);
        cleared.extra.insert("lab".into(), Value::Null);
        assert_eq!(merge(Some(&m), cleared, &known(&["a", "b"])).extra.get("lab"), Some(&Value::Null));
    }

    #[test]
    fn the_stored_format_reads_what_the_app_saved_before() {
        // A project as 0.10's UI kept it in browser storage.
        let v: Project = serde_json::from_str(
            r#"{"meta":{"id":"p","title":"Cat","updatedAt":1700000000000,"cover":"gen:g1","count":1},
                "items":[{"key":"k1","kind":"gen","genId":"g1","jobId":"j"},{"key":"k2","kind":"job","jobId":"j2","request":{"prompt":"x"}}],
                "selected":"k1","loras":["l"]}"#,
        )
        .unwrap();
        assert_eq!(
            v.items[1]
                .extra
                .get("request")
                .and_then(|r| r.get("prompt")),
            Some(&Value::from("x"))
        );
        let back = serde_json::to_value(&v).unwrap();
        assert_eq!(back["items"][0]["genId"], "g1");
        assert_eq!(back["meta"]["updatedAt"], 1700000000000.0);
    }

    #[test]
    fn projects_are_per_user_and_the_shared_list_stays_the_machines() {
        let s = ProjectStore::in_memory();
        // A pre-multi-user file (no owner key) is the machine's shared list.
        let (_, ch) = s
            .put("shared", project("shared", vec![img("a")]), &known(&[]), None)
            .unwrap();
        assert_eq!(ch.user, None);
        // A named account creates its own and sees exactly that one.
        s.put("alice", project("alice", vec![img("a")]), &known(&[]), Some("alice"))
            .unwrap();
        s.put("bob", project("bob", vec![img("b")]), &known(&[]), Some("bob"))
            .unwrap();
        assert_eq!(
            s.list_for(Some("alice"))
                .iter()
                .map(|p| p.meta.id.as_str())
                .collect::<Vec<_>>(),
            ["alice"]
        );
        assert_eq!(s.list_for(Some("bob")).len(), 1);
        // The machine and the operator see everything, as before.
        assert_eq!(s.list().len(), 3);
        // Anything that is not its own is an identical absence: shared, another's, absent.
        assert!(s.get_for("shared", Some("alice")).is_none());
        assert!(s.get_for("bob", Some("alice")).is_none());
        assert!(s.get_for("nope", Some("alice")).is_none());
        assert_eq!(s.get_for("alice", Some("alice")).unwrap().meta.id, "alice");
        assert_eq!(s.get_for("shared", None).unwrap().meta.id, "shared");
        // A named account cannot write into the shared list or into another's project —
        // and a client cannot claim ownership: there is no owner field to send.
        assert!(
            s.put("shared", project("shared", vec![img("a")]), &known(&[]), Some("alice"))
                .is_err()
        );
        assert!(
            s.put("bob", project("bob", vec![img("b")]), &known(&[]), Some("alice")).is_err()
        );
        assert!(s.delete("shared", 9.0, Some("alice")).is_err());
        assert!(s.delete("bob", 9.0, Some("alice")).is_err());
        // ... but it edits and deletes its own, and the owner travels in the change (the
        // event's filter), while a machine merge keeps it.
        let (_, ch) = s
            .put(
                "alice",
                project("alice", vec![img("a"), img("b")]),
                &known(&["a"]),
                Some("alice"),
            )
            .unwrap();
        assert_eq!(ch.user.as_deref(), Some("alice"));
        let (p, _) = s
            .put("alice", project("alice", vec![img("b")]), &known(&["a", "b"]), None)
            .unwrap();
        assert_eq!(p.meta.id, "alice");
        assert_eq!(s.owner_of("alice"), Some("alice".into()));
        // The import is namespaced the same way: a named account merges into and creates
        // its own, and leaves the shared list and another's untouched.
        let ch = s
            .import(
                vec![
                    project("shared", vec![img("a"), img("c")]),
                    project("bob", vec![img("b")]),
                    project("alice2", vec![img("d")]),
                ],
                Some("alice"),
            )
            .unwrap();
        assert_eq!(
            ch.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            ["alice2"]
        );
        assert_eq!(ch[0].user.as_deref(), Some("alice"));
        assert_eq!(keys(&s.get("shared").unwrap()), ["a"]); // the shared merge did not run
        assert_eq!(keys(&s.get("bob").unwrap()), ["b"]); // nor bob's
        assert_eq!(s.list_for(Some("alice")).len(), 2);
        // The machine's import still merges into the shared one and creates shared.
        let ch = s
            .import(
                vec![
                    project("shared", vec![img("a"), img("c")]),
                    project("m2", vec![img("e")]),
                ],
                None,
            )
            .unwrap();
        assert_eq!(
            ch.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            ["shared", "m2"]
        );
        assert!(ch.iter().all(|c| c.user.is_none()));
        assert_eq!(keys(&s.get("shared").unwrap()), ["a", "c"]);
        assert_eq!(s.owner_of("m2"), None);
        // A deleted project's owner entry goes with it (the tombstone stays).
        let ch = s.delete("alice", 10.0, Some("alice")).unwrap();
        assert_eq!(ch.user.as_deref(), Some("alice"));
        assert_eq!(s.owner_of("alice"), None);
        assert!(s.get("alice").is_none());
        assert!(s.delete("alice", 11.0, None).is_ok());
        // An account's projects go with the account (the operator deletes it).
        let ch = s.delete_user_projects("bob").unwrap();
        assert_eq!(ch.len(), 1);
        assert_eq!(ch[0].user.as_deref(), Some("bob"));
        assert!(s.get("bob").is_none());
        assert_eq!(s.owner_of("bob"), None);
    }

    #[test]
    fn the_file_export_stays_in_the_old_shape_for_one_version() {
        // D-032-1: while the export is on, the file the previous image reads keeps the
        // exact old serialisation — among it, a single-user world never gains the key.
        let dir = tempfile::tempdir().unwrap();
        let s = ProjectStore::open(dir.path(), true).unwrap();
        s.put("p", project("p", vec![img("a")]), &known(&[]), None)
            .unwrap();
        let text = std::fs::read_to_string(dir.path().join("projects.json")).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert!(!text.contains("\"owner\""), "the key must stay out: {text}");
        assert_eq!(v["projects"]["p"]["meta"]["id"], "p");
        assert_eq!(v["projects"]["p"]["items"][0]["key"], "a");
        // The export follows the data: a change lands in it, a removal tombstones in it.
        s.put("p", project("p", vec![img("a"), img("b")]), &known(&["a"]), None)
            .unwrap();
        let v: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("projects.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(v["projects"]["p"]["items"][1]["key"], "b");
        s.delete("p", 2.0, None).unwrap();
        let v: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("projects.json")).unwrap(),
        )
        .unwrap();
        assert!(v["projects"].get("p").is_none());
        assert!(v["deleted"].get("p").is_some());
    }

    #[test]
    fn the_file_imports_once_and_never_twice() {
        // D-032-1: a box whose list is still in the file gets it imported into the
        // database on the first open — and a second open (a restart) is a no-op, even if
        // the file is newer (the database is the source of truth from the import on).
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("projects.json"),
            r#"{
                "projects": {
                    "p": {"meta": {"id": "p", "title": "T", "updatedAt": 5, "cover": "gen:ga", "count": 1},
                          "items": [{"key": "a", "kind": "gen", "genId": "ga"}], "selected": "a", "rev": 3},
                    "alice-p": {"meta": {"id": "alice-p", "title": "A", "updatedAt": 6, "cover": "gen:gb", "count": 1},
                                "items": [{"key": "b", "kind": "gen", "genId": "gb"}], "selected": "b", "rev": 1}
                },
                "deleted": {"gone": 7},
                "owner": {"alice-p": "alice"}
            }"#,
        )
        .unwrap();
        let s = ProjectStore::open(dir.path(), true).unwrap();
        assert_eq!(s.list().len(), 2);
        assert_eq!(s.list_for(Some("alice")).len(), 1);
        assert_eq!(s.owner_of("alice-p"), Some("alice".into()));
        assert!(s.get("gone").is_none());
        // The pre-import bytes are the one-step rollback.
        assert!(dir.path().join("projects.json.pre-db").exists());
        // The export now reflects the database (the owner key present: a multi-user world).
        let v: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("projects.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(v["owner"]["alice-p"], "alice");
        // A restart never re-imports: even a file with more projects in it stays a
        // read-only copy of the database.
        std::fs::write(
            dir.path().join("projects.json"),
            r#"{"projects": {"fresh": {"meta": {"id": "fresh", "title": "F", "updatedAt": 1, "cover": null, "count": 0}, "items": [], "rev": 1}}, "deleted": {}}"#,
        )
        .unwrap();
        let again = ProjectStore::open(dir.path(), true).unwrap();
        assert_eq!(again.list().len(), 2, "the import ran once");
        assert!(again.get("fresh").is_none());
    }
}
