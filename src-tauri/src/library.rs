//! The library: books are copied into the app's folder on import (like Books.app)
//! and listed in a SQLite file with title, author, cover and reading position.
//!
//! Layout inside the app data folder:
//!   library/<id>.pdf | <id>.epub | <id>.cbz   the imported files
//!   library/<id>/                              an unpacked .cbr (its images)
//!   covers/<id>.jpg                            covers made by the UI (PDF, EPUB)
//!   library.sqlite

use crate::comic::{self, Source};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub type Result<T> = std::result::Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Book {
    pub id: i64,
    pub title: String,
    pub author: String,
    /// pdf | epub | cbz | cbr
    pub format: String,
    pub added_at: i64,
    pub opened_at: Option<i64>,
    /// page index (pdf, comics) or EPUB CFI where reading stopped
    pub position: Option<String>,
    /// 0.0 … 1.0
    pub progress: f64,
    pub has_cover: bool,
    /// title / author / cover already read from the file (PDF and EPUB do this in the UI)
    pub prepared: bool,
}

#[derive(Serialize, Debug, Default)]
pub struct ImportResult {
    pub books: Vec<Book>,
    pub errors: Vec<String>,
}

pub struct Library {
    conn: Connection,
    root: PathBuf,
}

const COLUMNS: &str = "id, title, author, format, added_at, opened_at, position, progress, has_cover, prepared";

fn book_from_row(r: &Row) -> rusqlite::Result<Book> {
    Ok(Book {
        id: r.get(0)?,
        title: r.get(1)?,
        author: r.get(2)?,
        format: r.get(3)?,
        added_at: r.get(4)?,
        opened_at: r.get(5)?,
        position: r.get(6)?,
        progress: r.get(7)?,
        has_cover: r.get(8)?,
        prepared: r.get(9)?,
    })
}

/// What a file really is, by its first bytes — many .cbr files are zips and vice versa.
fn sniff(path: &Path) -> Option<&'static str> {
    let mut head = [0u8; 8];
    let n = fs::File::open(path).ok()?.read(&mut head).ok()?;
    let head = &head[..n];
    if head.starts_with(b"%PDF") {
        Some("pdf")
    } else if head.starts_with(b"Rar!") {
        Some("rar")
    } else if head.starts_with(b"PK") {
        Some("zip")
    } else {
        None
    }
}

/// "My_Comic-01" → "My Comic-01"
fn title_from(path: &Path) -> String {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let title = stem.replace('_', " ");
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() { "Untitled".into() } else { title }
}

impl Library {
    pub fn open(root: &Path) -> Result<Self> {
        fs::create_dir_all(root.join("library")).map_err(err)?;
        fs::create_dir_all(root.join("covers")).map_err(err)?;
        let conn = Connection::open(root.join("library.sqlite")).map_err(err)?;
        conn.pragma_update(None, "journal_mode", "WAL").map_err(err)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS books (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                title      TEXT NOT NULL,
                author     TEXT NOT NULL DEFAULT '',
                format     TEXT NOT NULL,
                file       TEXT NOT NULL,          -- name inside library/
                added_at   INTEGER NOT NULL,
                opened_at  INTEGER,
                position   TEXT,
                progress   REAL NOT NULL DEFAULT 0,
                has_cover  INTEGER NOT NULL DEFAULT 0,
                prepared   INTEGER NOT NULL DEFAULT 0
            );",
        )
        .map_err(err)?;
        Ok(Self { conn, root: root.to_path_buf() })
    }

    /// Recently read first, then recently added.
    pub fn list(&self) -> Result<Vec<Book>> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT {COLUMNS} FROM books ORDER BY COALESCE(opened_at, added_at) DESC, id DESC"))
            .map_err(err)?;
        let rows = stmt.query_map([], book_from_row).map_err(err)?;
        rows.collect::<rusqlite::Result<_>>().map_err(err)
    }

    pub fn get(&self, id: i64) -> Result<Book> {
        self.conn
            .query_row(&format!("SELECT {COLUMNS} FROM books WHERE id = ?1"), params![id], book_from_row)
            .optional()
            .map_err(err)?
            .ok_or_else(|| "Book not found".into())
    }

    fn file_name(&self, id: i64) -> Result<String> {
        self.conn
            .query_row("SELECT file FROM books WHERE id = ?1", params![id], |r| r.get(0))
            .map_err(err)
    }

    /// Path of the imported file (or folder, for an unpacked .cbr).
    pub fn path(&self, id: i64) -> Result<PathBuf> {
        Ok(self.root.join("library").join(self.file_name(id)?))
    }

    pub fn cover_path(&self, id: i64) -> PathBuf {
        self.root.join("covers").join(format!("{id}.jpg"))
    }

    /// Where a comic's pages are.
    pub fn comic_source(&self, id: i64) -> Result<Source> {
        let book = self.get(id)?;
        let path = self.path(id)?;
        match book.format.as_str() {
            "cbz" => Ok(Source::Zip(path)),
            "cbr" => Ok(Source::Folder(path)),
            _ => Err("Not a comic".into()),
        }
    }

    /// Copies books into the library. Unsupported or broken files are reported, not fatal.
    pub fn import(&self, paths: &[PathBuf]) -> ImportResult {
        let mut result = ImportResult::default();
        for path in paths {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            match self.import_one(path) {
                Ok(book) => result.books.push(book),
                Err(e) => result.errors.push(format!("{name}: {e}")),
            }
        }
        result
    }

    fn import_one(&self, src: &Path) -> Result<Book> {
        let ext = src.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let kind = sniff(src);
        // trust the content over the extension for comics
        let format = match (ext.as_str(), kind) {
            ("pdf", _) | (_, Some("pdf")) => "pdf",
            ("epub", Some("zip")) => "epub",
            ("cbz" | "cbr", Some("zip")) => "cbz",
            ("cbz" | "cbr", Some("rar")) => "cbr",
            _ => return Err("not a PDF, EPUB, CBZ or CBR".into()),
        };
        let comic = matches!(format, "cbz" | "cbr");

        self.conn
            .execute(
                "INSERT INTO books (title, format, file, added_at, has_cover, prepared)
                 VALUES (?1, ?2, '', ?3, ?4, ?4)",
                params![title_from(src), format, now(), comic],
            )
            .map_err(err)?;
        let id = self.conn.last_insert_rowid();

        let stored = if format == "cbr" { id.to_string() } else { format!("{id}.{format}") };
        let target = self.root.join("library").join(&stored);
        let copied = if format == "cbr" {
            comic::unpack_rar(src, &target).and_then(|n| if n == 0 { Err("no pages inside".into()) } else { Ok(()) })
        } else {
            fs::copy(src, &target).map(|_| ()).map_err(err)
        };
        let copied = copied.and_then(|_| {
            // a comic must have at least one page we can show
            if comic {
                let source = if format == "cbz" { Source::Zip(target.clone()) } else { Source::Folder(target.clone()) };
                let pages = comic::load(source)?;
                if pages.names.is_empty() {
                    return Err("no pages inside".into());
                }
            }
            Ok(())
        });

        if let Err(e) = copied {
            let _ = fs::remove_dir_all(&target);
            let _ = fs::remove_file(&target);
            let _ = self.conn.execute("DELETE FROM books WHERE id = ?1", params![id]);
            return Err(e);
        }
        self.conn.execute("UPDATE books SET file = ?1 WHERE id = ?2", params![stored, id]).map_err(err)?;
        self.get(id)
    }

    /// Title, author and cover found by the UI (PDF / EPUB metadata, first page).
    pub fn set_details(&self, id: i64, title: Option<&str>, author: Option<&str>, cover_jpeg: Option<&[u8]>) -> Result<Book> {
        if let Some(bytes) = cover_jpeg {
            fs::write(self.cover_path(id), bytes).map_err(err)?;
        }
        self.conn
            .execute(
                "UPDATE books SET
                    title     = COALESCE(NULLIF(TRIM(?1), ''), title),
                    author    = COALESCE(?2, author),
                    has_cover = has_cover OR ?3,
                    prepared  = 1
                 WHERE id = ?4",
                params![title, author.map(str::trim), cover_jpeg.is_some(), id],
            )
            .map_err(err)?;
        self.get(id)
    }

    pub fn set_progress(&self, id: i64, position: &str, progress: f64) -> Result<()> {
        self.conn
            .execute(
                "UPDATE books SET position = ?1, progress = ?2, opened_at = ?3 WHERE id = ?4",
                params![position, progress.clamp(0.0, 1.0), now(), id],
            )
            .map_err(err)?;
        Ok(())
    }

    /// Removes a book; its stored copy and cover go to the Trash, so it can be recovered.
    pub fn remove(&self, id: i64) -> Result<()> {
        let path = self.path(id)?;
        if path.exists() {
            trash::delete(&path).map_err(err)?;
        }
        let cover = self.cover_path(id);
        if cover.exists() {
            let _ = fs::remove_file(cover);
        }
        self.conn.execute("DELETE FROM books WHERE id = ?1", params![id]).map_err(err)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comic::tests::make_cbz;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rebook-lib-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn imports_by_content_and_rejects_the_rest() {
        let dir = temp("import");
        let lib = Library::open(&dir.join("app")).unwrap();

        make_cbz(&dir.join("My_Comic_01.cbr"), &[("1.png", 800, 1200)]); // a zip with a .cbr name
        fs::write(dir.join("paper.pdf"), b"%PDF-1.7 ...").unwrap();
        fs::write(dir.join("notes.txt"), b"hello").unwrap();
        make_cbz(&dir.join("empty.cbz"), &[]);

        let result = lib.import(&[
            dir.join("My_Comic_01.cbr"),
            dir.join("paper.pdf"),
            dir.join("notes.txt"),
            dir.join("empty.cbz"),
        ]);
        assert_eq!(result.books.len(), 2, "errors: {:?}", result.errors);
        assert_eq!(result.errors.len(), 2);

        let comic = &result.books[0];
        assert_eq!(comic.format, "cbz");
        assert_eq!(comic.title, "My Comic 01");
        assert!(comic.prepared && comic.has_cover);
        assert!(lib.path(comic.id).unwrap().exists());

        let pdf = &result.books[1];
        assert_eq!(pdf.format, "pdf");
        assert!(!pdf.prepared, "PDF details are filled in by the UI");

        let updated = lib.set_details(pdf.id, Some("A Paper"), Some("Ann"), Some(b"jpeg")).unwrap();
        assert_eq!((updated.title.as_str(), updated.author.as_str()), ("A Paper", "Ann"));
        assert!(updated.has_cover && updated.prepared);

        lib.set_progress(pdf.id, "3", 0.5).unwrap();
        let listed = lib.list().unwrap();
        assert_eq!(listed[0].id, pdf.id, "just-read book comes first");
        assert_eq!(listed[0].position.as_deref(), Some("3"));
        assert_eq!(fs::read_dir(dir.join("app/library")).unwrap().count(), 2, "failed imports leave nothing behind");
        fs::remove_dir_all(&dir).unwrap();
    }
}
