//! Clipboard commands - native pasteboard writes.
//!
//! Note copies go through here instead of `navigator.clipboard`: WKWebView
//! routes JS clipboard writes through its own sandboxed layer, which
//! clipboard managers (Paste, Maccy, ...) don't reliably observe. arboard
//! writes the OS pasteboard directly. Every write hops to the main thread -
//! AppKit's NSPasteboard is not thread-safe and off-main writes race
//! WebKit's pasteboard monitoring (tauri-apps/plugins-workspace#3205).

use tauri::AppHandle;

#[tauri::command]
pub async fn clipboard_write_text(app: AppHandle, text: String) -> Result<(), String> {
    write_on_main(app, move |cb| cb.set_text(text)).await
}

/// Write `html` with a plain-text `alt` fallback so non-rich paste targets
/// still receive something sensible.
#[tauri::command]
pub async fn clipboard_write_html(app: AppHandle, html: String, alt: String) -> Result<(), String> {
    write_on_main(app, move |cb| cb.set_html(html, Some(alt))).await
}

async fn write_on_main<F>(app: AppHandle, op: F) -> Result<(), String>
where
    F: FnOnce(&mut arboard::Clipboard) -> Result<(), arboard::Error> + Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let res = arboard::Clipboard::new()
            .and_then(|mut cb| op(&mut cb))
            .map_err(|e| e.to_string());
        let _ = tx.send(res);
    })
    .map_err(|e| e.to_string())?;
    rx.await
        .map_err(|e| format!("clipboard write dropped: {e}"))?
}
