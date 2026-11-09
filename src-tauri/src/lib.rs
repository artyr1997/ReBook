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

/// The window starts hidden (tauri.conf.json) and fades in once the interface has drawn itself
/// (window_ready), so the app opens softly instead of appearing empty.
#[cfg(target_os = "macos")]
fn fade_in(window: &tauri::Window) {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    if window.is_visible().unwrap_or(true) {
        return;
    }
    let Ok(ns_window) = window.ns_window() else {
        let _ = window.show();
        return;
    };
    let ns_window = ns_window as *mut AnyObject;
    // SAFETY: the live NSWindow of `window`, on the main thread (see reveal).
    unsafe {
        let _: () = msg_send![ns_window, setAlphaValue: 0.0f64];
    }
    let _ = window.show();
    let _ = window.set_focus();
    unsafe {
        let _: () = msg_send![class!(NSAnimationContext), beginGrouping];
        let context: *mut AnyObject = msg_send![class!(NSAnimationContext), currentContext];
        let _: () = msg_send![context, setDuration: 0.3f64];
        let name: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: c"easeOut".as_ptr()];
        let timing: *mut AnyObject = msg_send![class!(CAMediaTimingFunction), functionWithName: name];
        let _: () = msg_send![context, setTimingFunction: timing];
        let animator: *mut AnyObject = msg_send![ns_window, animator];
        let _: () = msg_send![animator, setAlphaValue: 1.0f64];
        let _: () = msg_send![class!(NSAnimationContext), endGrouping];
    }
}

/// Fades the window in on the main thread; `after` waits first (the fallback, should the interface never say it is ready).
fn reveal(window: &tauri::Window, after: std::time::Duration) {
    let window = window.clone();
    std::thread::spawn(move || {
        std::thread::sleep(after);
        let target = window.clone();
        let _ = window.run_on_main_thread(move || {
            #[cfg(target_os = "macos")]
            fade_in(&target);
            #[cfg(not(target_os = "macos"))]
            let _ = target.show();
        });
    });
}

/// The interface has drawn itself (main.ts): the window can fade in.
#[tauri::command]
fn window_ready(webview: tauri::Webview) {
    // the webview's window: ReSurf's holds its tabs' webviews too
    reveal(&webview.window(), std::time::Duration::ZERO);
}

/// ReDesk's Hide / Show Windows (a distributed notification, "hide" or "show"): the windows fade out
/// and then the app hides, or the app shows again with its windows fading in.
#[cfg(target_os = "macos")]
fn listen_for_desk_fades() {
    use block2::RcBlock;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use std::ffi::CStr;

    /// every window of the app animated to `alpha`, then `done`
    unsafe fn animate(alpha: f64, done: Option<&block2::Block<dyn Fn()>>) {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let windows: *mut AnyObject = msg_send![app, windows];
        let count: usize = msg_send![windows, count];
        let _: () = msg_send![class!(NSAnimationContext), beginGrouping];
        let context: *mut AnyObject = msg_send![class!(NSAnimationContext), currentContext];
        let _: () = msg_send![context, setDuration: 0.25f64];
        if let Some(done) = done {
            let _: () = msg_send![context, setCompletionHandler: done];
        }
        for i in 0..count {
            let window: *mut AnyObject = msg_send![windows, objectAtIndex: i];
            let animator: *mut AnyObject = msg_send![window, animator];
            let _: () = msg_send![animator, setAlphaValue: alpha];
        }
        let _: () = msg_send![class!(NSAnimationContext), endGrouping];
    }

    /// every window's alpha set at once
    unsafe fn set_alpha(alpha: f64) {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let windows: *mut AnyObject = msg_send![app, windows];
        let count: usize = msg_send![windows, count];
        for i in 0..count {
            let window: *mut AnyObject = msg_send![windows, objectAtIndex: i];
            let _: () = msg_send![window, setAlphaValue: alpha];
        }
    }

    let handler = RcBlock::new(|note: *mut AnyObject| {
        // SAFETY: on the main queue, with AppKit's own objects
        unsafe {
            let object: *mut AnyObject = msg_send![note, object];
            if object.is_null() {
                return;
            }
            let text: *const std::os::raw::c_char = msg_send![object, UTF8String];
            if text.is_null() {
                return;
            }
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let hidden: bool = msg_send![app, isHidden];
            match CStr::from_ptr(text).to_bytes() {
                b"hide" if !hidden => {
                    let done = RcBlock::new(|| {
                        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
                        let _: () = msg_send![app, hide: std::ptr::null::<AnyObject>()];
                        set_alpha(1.0);
                    });
                    animate(0.0, Some(&done));
                }
                b"show" if hidden => {
                    set_alpha(0.0);
                    let _: () = msg_send![app, unhideWithoutActivation];
                    animate(1.0, None);
                }
                _ => {}
            }
        }
    });
    // SAFETY: the center copies the block and keeps the observer for the app's life
    unsafe {
        let center: *mut AnyObject = msg_send![class!(NSDistributedNotificationCenter), defaultCenter];
        let name: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: c"com.artursavytskyi.re.windows".as_ptr()];
        let queue: *mut AnyObject = msg_send![class!(NSOperationQueue), mainQueue];
        let _: *mut AnyObject = msg_send![center, addObserverForName: name, object: std::ptr::null::<AnyObject>(),
                                          queue: queue, usingBlock: &*handler];
    }
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
            #[cfg(target_os = "macos")]
            listen_for_desk_fades();
            // ~/Library/Application Support/<identifier>/ on macOS
            let root = app.path().app_data_dir()?;
            let library = Library::open(&root)?;
            app.manage(AppState { library: Mutex::new(library), pages: Mutex::new(HashMap::new()) });

            #[cfg(target_os = "macos")]
            if let Some(window) = app.get_webview_window("main") {
                round_window(&window);
                reveal(&window.as_ref().window(), std::time::Duration::from_secs(3));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            window_ready,
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
