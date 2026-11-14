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

/// How long the windows take to fade out (hiding, closing) and in (showing again).
#[cfg(target_os = "macos")]
const FADE_SECONDS: f64 = 0.4;

/// `windows` animated to `alpha` over FADE_SECONDS, then `done`.
#[cfg(target_os = "macos")]
unsafe fn fade_windows(windows: &[*mut objc2::runtime::AnyObject], alpha: f64, done: Option<&block2::Block<dyn Fn()>>) {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    let _: () = msg_send![class!(NSAnimationContext), beginGrouping];
    let context: *mut AnyObject = msg_send![class!(NSAnimationContext), currentContext];
    let _: () = msg_send![context, setDuration: FADE_SECONDS];
    let name: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: c"easeInEaseOut".as_ptr()];
    let timing: *mut AnyObject = msg_send![class!(CAMediaTimingFunction), functionWithName: name];
    let _: () = msg_send![context, setTimingFunction: timing];
    if let Some(done) = done {
        let _: () = msg_send![context, setCompletionHandler: done];
    }
    for &window in windows {
        let animator: *mut AnyObject = msg_send![window, animator];
        let _: () = msg_send![animator, setAlphaValue: alpha];
    }
    let _: () = msg_send![class!(NSAnimationContext), endGrouping];
}

/// all of the app's NSWindows
#[cfg(target_os = "macos")]
unsafe fn app_windows() -> Vec<*mut objc2::runtime::AnyObject> {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
    let windows: *mut AnyObject = msg_send![app, windows];
    let count: usize = msg_send![windows, count];
    (0..count).map(|i| msg_send![windows, objectAtIndex: i]).collect()
}

/// NSApplication's own hide:, kept to be called once the windows have faded out
#[cfg(target_os = "macos")]
static HIDE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// hide: (⌘H, the Hide menu item, ReDesk's Hide / Show Windows): the windows fade out first
#[cfg(target_os = "macos")]
unsafe extern "C-unwind" fn fading_hide(this: *mut objc2::runtime::AnyObject, sel: objc2::runtime::Sel, sender: *mut objc2::runtime::AnyObject) {
    use block2::RcBlock;
    let this = this as usize;
    let sender = sender as usize;
    let done = RcBlock::new(move || {
        // SAFETY: HIDE holds NSApplication's hide:, which takes these arguments
        unsafe {
            let hide: unsafe extern "C-unwind" fn(*mut objc2::runtime::AnyObject, objc2::runtime::Sel, *mut objc2::runtime::AnyObject) =
                std::mem::transmute(HIDE.load(std::sync::atomic::Ordering::Relaxed));
            hide(this as *mut _, sel, sender as *mut _);
            // hidden now: back to whole, for when the app shows again (which fades them in)
            for window in app_windows() {
                let _: () = objc2::msg_send![window, setAlphaValue: 1.0f64];
            }
        }
    });
    // SAFETY: on the main thread, as hide: is
    unsafe { fade_windows(&app_windows(), 0.0, Some(&done)) };
}

/// Windows fade out when the app hides and when one closes, and fade in when the app shows again.
/// ReDesk's Hide / Show Windows and Close All Windows ask with a distributed notification, "hide", "show" or "close".
#[cfg(target_os = "macos")]
fn fade_windows_in_and_out() {
    use block2::RcBlock;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send, sel};
    use std::ffi::CStr;

    // SAFETY: AppKit's own objects, on the main thread (setup); hide: keeps its signature
    unsafe {
        let method = objc2::ffi::class_getInstanceMethod(class!(NSApplication), sel!(hide:));
        if !method.is_null() {
            let ours: objc2::runtime::Imp = std::mem::transmute(fading_hide as *const ());
            // only once: a second time would keep fading_hide as the "original"
            if HIDE.load(std::sync::atomic::Ordering::Relaxed) == 0 {
                if let Some(old) = objc2::ffi::method_setImplementation(method, ours) {
                    HIDE.store(old as usize, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }

        let center: *mut AnyObject = msg_send![class!(NSNotificationCenter), defaultCenter];
        let null = std::ptr::null::<AnyObject>();
        let will = RcBlock::new(|_: *mut AnyObject| {
            for window in app_windows() {
                let _: () = msg_send![window, setAlphaValue: 0.0f64];
            }
        });
        let did = RcBlock::new(|_: *mut AnyObject| fade_windows(&app_windows(), 1.0, None));
        let will_name: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: c"NSApplicationWillUnhideNotification".as_ptr()];
        let did_name: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: c"NSApplicationDidUnhideNotification".as_ptr()];
        let _: *mut AnyObject = msg_send![center, addObserverForName: will_name, object: null, queue: null, usingBlock: &*will];
        let _: *mut AnyObject = msg_send![center, addObserverForName: did_name, object: null, queue: null, usingBlock: &*did];

        let desk = RcBlock::new(|note: *mut AnyObject| {
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
            let said = CStr::from_ptr(text).to_bytes();
            // "hide <pid>": this app alone (ReDesk's own desktops)
            let mine = format!("hide {}", std::process::id());
            if said == mine.as_bytes() {
                if !hidden {
                    let _: () = msg_send![app, hide: null];
                }
                return;
            }
            match said {
                b"hide" if !hidden => {
                    let _: () = msg_send![app, hide: null];
                }
                b"show" if hidden => {
                    let _: () = msg_send![app, unhideWithoutActivation];
                }
                b"close" => {
                    let windows = app_windows();
                    let shown: Vec<_> = windows.iter().copied().filter(|&w| msg_send![w, isVisible]).collect();
                    let gone = windows.iter().map(|&w| w as usize).collect::<Vec<_>>();
                    let done = RcBlock::new(move || {
                        for &window in &gone {
                            let _: () = msg_send![window as *mut AnyObject, close];
                        }
                    });
                    fade_windows(&shown, 0.0, Some(&done));
                }
                _ => {}
            }
        });
        let distributed: *mut AnyObject = msg_send![class!(NSDistributedNotificationCenter), defaultCenter];
        let name: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: c"com.artursavytskyi.re.windows".as_ptr()];
        let queue: *mut AnyObject = msg_send![class!(NSOperationQueue), mainQueue];
        let _: *mut AnyObject = msg_send![distributed, addObserverForName: name, object: null, queue: queue, usingBlock: &*desk];
    }
}

/// The app, for ReDesk's word on which corners to round (listen_for_desk_corners).
#[cfg(target_os = "macos")]
static DESK_APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

/// ReDesk's Flush tiling: the windows side by side as one, so only the outer corners of them all are
/// round. ReDesk says, for each window, which corners (1 top left, 2 top right, 4 bottom left, 8 bottom
/// right) and which of its edges touch another window (1 top, 2 right, 4 bottom, 8 left): the window
/// rounds only those corners (no window has a rim or a shadow anyway).
#[cfg(target_os = "macos")]
fn listen_for_desk_corners(app: &tauri::AppHandle) {
    use block2::RcBlock;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use tauri::Manager;

    let _ = DESK_APP.set(app.clone());
    let handler = RcBlock::new(|note: *mut AnyObject| {
        // SAFETY: on the main queue; the userInfo ReDesk sends holds numbers only
        unsafe {
            let info: *mut AnyObject = msg_send![note, userInfo];
            if info.is_null() {
                return;
            }
            let number = |key: &std::ffi::CStr| -> f64 {
                let key: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: key.as_ptr()];
                let value: *mut AnyObject = msg_send![info, objectForKey: key];
                if value.is_null() { -1.0 } else { msg_send![value, doubleValue] }
            };
            if number(c"pid") as u32 != std::process::id() {
                return;
            }
            let corners = number(c"corners") as u32;
            // pinned by ReDesk (⌃⌥P): the page writes P in its top left corner
            let pinned = number(c"pinned") == 1.0;
            let Some(app) = DESK_APP.get() else { return };
            // which of the app's windows it is: the one with that frame
            let near = |a: f64, b: f64| (a - b).abs() < 24.0;
            for window in app.webview_windows().into_values() {
                let scale = window.scale_factor().unwrap_or(1.0);
                let (Ok(at), Ok(size)) = (window.outer_position(), window.outer_size()) else { continue };
                if near(at.x as f64 / scale, number(c"x")) && near(at.y as f64 / scale, number(c"y"))
                    && near(size.width as f64 / scale, number(c"w")) && near(size.height as f64 / scale, number(c"h"))
                {
                    set_corners(&window, corners);
                    let _ = tauri::Emitter::emit_to(app, window.label(), "pinned", pinned);
                }
            }
        }
    });
    // SAFETY: AppKit's own objects, on the main thread (setup); the center keeps the block
    unsafe {
        let center: *mut AnyObject = msg_send![class!(NSDistributedNotificationCenter), defaultCenter];
        let name: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: c"com.artursavytskyi.re.corners".as_ptr()];
        let queue: *mut AnyObject = msg_send![class!(NSOperationQueue), mainQueue];
        let _: *mut AnyObject = msg_send![center, addObserverForName: name, object: std::ptr::null::<AnyObject>(),
                                          queue: queue, usingBlock: &*handler];
    }
}

/// Only `corners` round (the view's layer). No rim and no shadow, alone or not.
#[cfg(target_os = "macos")]
fn set_corners(window: &tauri::WebviewWindow, corners: u32) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    let _ = window.with_webview(move |platform| {
        let view = platform.inner() as *mut AnyObject;
        if view.is_null() {
            return;
        }
        // SAFETY: the live WKWebView, on the main thread
        unsafe {
            let layer: *mut AnyObject = msg_send![view, layer];
            if layer.is_null() {
                return;
            }
            // the layer's own corners (CACornerMask: min X min Y 1, max X min Y 2, min X max Y 4, max X max Y 8);
            // its min Y is at the top when its contents are flipped
            let flipped: bool = msg_send![layer, contentsAreFlipped];
            let (top_left, top_right, bottom_left, bottom_right) = if flipped { (1, 2, 4, 8) } else { (4, 8, 1, 2) };
            let mut mask: usize = 0;
            if corners & 1 != 0 { mask |= top_left; }
            if corners & 2 != 0 { mask |= top_right; }
            if corners & 4 != 0 { mask |= bottom_left; }
            if corners & 8 != 0 { mask |= bottom_right; }
            let _: () = msg_send![layer, setMaskedCorners: mask];
        }
    });
}

/// A window asked to close fades out first, then goes.
fn fade_on_close(window: &tauri::Window, event: &tauri::WindowEvent) {
    #[cfg(target_os = "macos")]
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        let Ok(ns_window) = window.ns_window() else { return };
        api.prevent_close();
        let target = window.clone();
        let done = block2::RcBlock::new(move || {
            let _ = target.destroy();
        });
        // SAFETY: the live NSWindow of `window`, on the main thread (window events are)
        unsafe { fade_windows(&[ns_window as *mut objc2::runtime::AnyObject], 0.0, Some(&done)) };
    }
}


// ── more windows: ⌘N ──

/// Windows opened so far (the first is "main"): the next one's label.
static WINDOW_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);

/// ⌘N: another window, as the first one is (tauri.conf.json): the size of the one in front, a little
/// lower and to the right of it. It fades in once its interface has drawn itself (window_ready).
#[tauri::command]
async fn new_window(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let n = WINDOW_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    let mut config = app.config().app.windows.first().cloned().ok_or("No window to copy")?;
    config.label = format!("w{n}");
    let windows = app.webview_windows();
    let front = windows.values().find(|w| w.is_focused().unwrap_or(false)).or_else(|| windows.values().next());
    let mut place = None;
    if let Some(front) = front {
        let scale = front.scale_factor().unwrap_or(1.0);
        if let (Ok(at), Ok(size)) = (front.outer_position(), front.inner_size()) {
            config.center = false;
            config.width = size.width as f64 / scale;
            config.height = size.height as f64 / scale;
            place = Some((at.x as f64 / scale + 28.0, at.y as f64 / scale + 28.0));
        }
    }
    let mut builder = tauri::WebviewWindowBuilder::from_config(&app, &config).map_err(|e| e.to_string())?;
    if let Some((x, y)) = place {
        builder = builder.position(x, y);
    }
    let window = builder.build().map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    round_window(&window);
    reveal(&window.as_ref().window(), std::time::Duration::from_secs(3));
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .on_window_event(fade_on_close)
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("rebook", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_owned();
            // read files off the main thread so big books don't freeze the window
            std::thread::spawn(move || responder.respond(serve(&app, &path)));
        })
        .setup(|app| {
            #[cfg(target_os = "macos")]
            fade_windows_in_and_out();
            #[cfg(target_os = "macos")]
            listen_for_desk_corners(app.handle());
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
            new_window,
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
