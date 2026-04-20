//! macOS menu-bar tray.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, Runtime,
};

use crate::storage::types::Project;

const TRAY_ID: &str = "atlas-menu-bar";

/// 22pt monochrome template icon (white "A" + summit dot on transparent).
const TRAY_TEMPLATE_PNG: &[u8] = include_bytes!("../icons/tray/atlasTemplate@2x.png");

/// `menu_event_id → project_id` map, so clicking a recent project item
fn recents_map() -> &'static Mutex<HashMap<String, String>> {
    static MAP: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn install<R: Runtime>(app: &AppHandle<R>, recents: Vec<Project>) -> tauri::Result<()> {
    if app.tray_by_id(TRAY_ID).is_some() {
        rebuild_menu(app, recents)?;
        return Ok(());
    }

    let menu = build_menu(app, &recents)?;
    store_recents(&recents);

    let icon = Image::from_bytes(TRAY_TEMPLATE_PNG)
        .map_err(|e| tauri::Error::AssetNotFound(format!("tray template icon: {e}")))?;

    let _tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .icon_as_template(true)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| handle_menu_event(app, event.id.as_ref()))
        .build(app)?;

    Ok(())
}

pub fn set_enabled<R: Runtime>(app: &AppHandle<R>, enabled: bool, recents: Vec<Project>) {
    if enabled {
        if let Err(e) = install(app, recents) {
            tracing::warn!(error = %e, "tray: install failed");
        }
    } else {
        app.remove_tray_by_id(TRAY_ID);
        recents_map().lock().ok().map(|mut m| m.clear());
    }
}

fn rebuild_menu<R: Runtime>(app: &AppHandle<R>, recents: Vec<Project>) -> tauri::Result<()> {
    let menu = build_menu(app, &recents)?;
    store_recents(&recents);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(menu))?;
    }
    Ok(())
}

fn build_menu<R: Runtime>(
    app: &AppHandle<R>,
    recents: &[Project],
) -> tauri::Result<tauri::menu::Menu<R>> {
    let mut builder = MenuBuilder::new(app)
        .item(
            &MenuItemBuilder::new("Atlas")
                .id("tray::header")
                .enabled(false)
                .build(app)?,
        )
        .item(&PredefinedMenuItem::separator(app)?);

    if recents.is_empty() {
        builder = builder.item(
            &MenuItemBuilder::new("No recent projects")
                .id("tray::no-recents")
                .enabled(false)
                .build(app)?,
        );
    } else {
        let mut sub = SubmenuBuilder::new(app, "Recent projects");
        for (i, p) in recents.iter().take(10).enumerate() {
            let id = format!("tray::recent::{i}");
            sub = sub.item(&MenuItemBuilder::new(&p.name).id(&id).build(app)?);
        }
        builder = builder.item(&sub.build()?);
    }

    builder = builder
        .item(&PredefinedMenuItem::separator(app)?)
        .item(
            &MenuItemBuilder::new("Show Atlas")
                .id("tray::show")
                .build(app)?,
        )
        .item(&MenuItemBuilder::new("Quit").id("tray::quit").build(app)?);

    builder.build()
}

fn store_recents(recents: &[Project]) {
    if let Ok(mut map) = recents_map().lock() {
        map.clear();
        for (i, p) in recents.iter().take(10).enumerate() {
            map.insert(format!("tray::recent::{i}"), p.id.clone());
        }
    }
}

fn handle_menu_event<R: Runtime>(app: &AppHandle<R>, id: &str) {
    match id {
        "tray::quit" => {
            app.exit(0);
        }
        "tray::show" => show_main_window(app),
        id if id.starts_with("tray::recent::") => {
            let project_id = recents_map().lock().ok().and_then(|m| m.get(id).cloned());
            if let Some(pid) = project_id {
                show_main_window(app);
                let _ = app.emit("tray:open-project", pid);
            }
        }
        _ => {}
    }
}

fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }
}
