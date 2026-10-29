//! ReBook: a quiet reader for PDF, EPUB and comics in the ReNote style.
//! Rust keeps the library (imported copies + SQLite) and serves book files and
//! comic pages to the Svelte UI over the `rebook://` protocol:
//!   rebook://localhost/file/<id>          the PDF / EPUB itself
//!   rebook://localhost/page/<id>/<index>  one comic page image
//!   rebook://localhost/cover/<id>         the cover

mod comic;
mod library;

use base64::Engine;
use comic::{PageSize, Pages};
use library::{Book, ImportResult, Library};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::http::{Response, StatusCode};
use tauri::{Manager, State};

struct AppState {
    library: Mutex<Library>,
    /// comic page lists, read once per book
    pages: Mutex<HashMap<i64, Arc<Pages>>>,
}

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl AppState {
    fn comic_pages(&self, id: i64) -> CmdResult<Arc<Pages>> {
        if let Some(p) = self.pages.lock().map_err(err)?.get(&id) {
            return Ok(p.clone());
        }
        let source = self.library.lock().map_err(err)?.comic_source(id)?;
        let pages = Arc::new(comic::load(source)?);
        self.pages.lock().map_err(err)?.insert(id, pages.clone());
        Ok(pages)
    }
}

#[tauri::command]
fn list_books(state: State<AppState>) -> CmdResult<Vec<Book>> {
    state.library.lock().map_err(err)?.list()
}

#[tauri::command]
fn import_books(state: State<AppState>, paths: Vec<String>) -> CmdResult<ImportResult> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    Ok(state.library.lock().map_err(err)?.import(&paths))
}

/// Saves what the UI read from a PDF / EPUB: title, author and a cover as a JPEG data URL.
#[tauri::command]
fn set_details(
    state: State<AppState>,
    id: i64,
    title: Option<String>,
    author: Option<String>,
    cover: Option<String>,
) -> CmdResult<Book> {
    let cover = cover
        .and_then(|url| url.split_once(',').map(|(_, data)| data.to_owned()))
        .map(|data| base64::engine::general_purpose::STANDARD.decode(data).map_err(err))
        .transpose()?;
    state.library.lock().map_err(err)?.set_details(id, title.as_deref(), author.as_deref(), cover.as_deref())
}

#[tauri::command]
fn set_progress(state: State<AppState>, id: i64, position: String, progress: f64) -> CmdResult<()> {
    state.library.lock().map_err(err)?.set_progress(id, &position, progress)
}

#[tauri::command]
fn remove_book(state: State<AppState>, id: i64) -> CmdResult<()> {
    state.pages.lock().map_err(err)?.remove(&id);
    state.library.lock().map_err(err)?.remove(id)
}

/// Manga mode: right-to-left reading for this book.
#[tauri::command]
fn set_rtl(state: State<AppState>, id: i64, rtl: bool) -> CmdResult<()> {
    state.library.lock().map_err(err)?.set_rtl(id, rtl)
}

/// Sizes of every comic page, so the reader can lay out spreads.
#[tauri::command]
fn comic_pages(state: State<AppState>, id: i64) -> CmdResult<Vec<PageSize>> {
    Ok(state.comic_pages(id)?.sizes.clone())
}

fn respond(status: StatusCode, mime: &str, body: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("Content-Type", mime)
        // the UI runs on another origin (tauri://localhost) and fetches PDF / EPUB bytes
        .header("Access-Control-Allow-Origin", "*")
        .header("Cache-Control", "max-age=3600")
        .body(body)
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

fn not_found(message: String) -> Response<Vec<u8>> {
    respond(StatusCode::NOT_FOUND, "text/plain", message.into_bytes())
}

/// Serves `rebook://localhost/<kind>/<id>[/<index>]`.
fn serve(app: &tauri::AppHandle, path: &str) -> Response<Vec<u8>> {
    let state = app.state::<AppState>();
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let id = parts.get(1).and_then(|s| s.parse::<i64>().ok());
    let result: CmdResult<(Vec<u8>, &str)> = (|| match (parts.first().copied(), id) {
        (Some("file"), Some(id)) => {
            let (path, format) = {
                let lib = state.library.lock().map_err(err)?;
                (lib.path(id)?, lib.get(id)?.format)
            };
            let mime = if format == "pdf" { "application/pdf" } else { "application/epub+zip" };
            Ok((std::fs::read(path).map_err(err)?, mime))
        }
        (Some("page"), Some(id)) => {
            let index = parts.get(2).and_then(|s| s.parse::<usize>().ok()).ok_or("No page index")?;
            state.comic_pages(id)?.read(index)
        }
        (Some("cover"), Some(id)) => {
            let cover = state.library.lock().map_err(err)?.cover_path(id);
            if cover.exists() {
                Ok((std::fs::read(cover).map_err(err)?, "image/jpeg"))
            } else {
                // comics: the first page is the cover
                state.comic_pages(id)?.read(0)
            }
        }
        _ => Err("Unknown address".into()),
    })();
    match result {
        Ok((body, mime)) => respond(StatusCode::OK, mime, body),
        Err(e) => not_found(e),
    }
}

/// No title bar at all (decorations are off in tauri.conf.json): macOS rims every titled window with
/// a light line that can't be turned off. The window is clear instead, its page is rounded here like a
/// window's corners, and the UI draws its own close / minimise / zoom buttons. No shadow either
/// (tauri.conf.json).
#[cfg(target_os = "macos")]
fn round_window(window: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    let _ = window.with_webview(|platform| {
        let view = platform.inner() as *mut AnyObject;
        if view.is_null() {
            return;
        }
        // SAFETY: `inner()` is the live WKWebView (an NSView); this runs on the main thread.
        unsafe {
            let _: () = msg_send![view, setWantsLayer: true];
            let layer: *mut AnyObject = msg_send![view, layer];
            if !layer.is_null() {
                let _: () = msg_send![layer, setCornerRadius: 12.0f64];
                let _: () = msg_send![layer, setMasksToBounds: true];
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("rebook", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_owned();
            // read files off the main thread so big books don't freeze the window
            std::thread::spawn(move || responder.respond(serve(&app, &path)));
        })
        .setup(|app| {
            // ~/Library/Application Support/<identifier>/ on macOS
            let root = app.path().app_data_dir()?;
            let library = Library::open(&root)?;
            app.manage(AppState { library: Mutex::new(library), pages: Mutex::new(HashMap::new()) });

            #[cfg(target_os = "macos")]
            if let Some(window) = app.get_webview_window("main") {
                round_window(&window);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_books,
            import_books,
            set_details,
            set_progress,
            remove_book,
            set_rtl,
            comic_pages,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ReBook");
}
