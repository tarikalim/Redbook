// Release builds on Windows are GUI apps: no console window behind the editor.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod database;
mod db;
mod editor;
mod import;
mod mcp;
mod mem;
mod model;
mod search;
mod sidebar;
mod store;
mod sync;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;

gpui_kit::actions!(redbook, [Quit, ToggleTheme]);

/// Runs a CLI subcommand and exits.
fn exit_with(r: anyhow::Result<()>) -> ! {
    if let Err(e) = r {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
    std::process::exit(0)
}

/// GUI-subsystem builds have no console; reuse the parent terminal's for CLI output.
#[cfg(windows)]
fn attach_console() {
    unsafe extern "system" {
        fn AttachConsole(pid: u32) -> i32;
    }
    unsafe { AttachConsole(u32::MAX) };
}

#[cfg(not(windows))]
fn attach_console() {}

fn main() {
    mem::ensure_space_efficient_malloc();
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("mcp") => exit_with(mcp::run()),
        Some("sync") => {
            attach_console();
            exit_with(sync::cli(&args[2..]))
        }
        Some("import") => {
            attach_console();
            exit_with(import::run(&args[2..]))
        }
        _ => {}
    }
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            mem::cap_glyph_cache(0);
            db::init(store::Store::open_default().expect("open database"), cx);
            sync::start(cx);

            // Bind keys before set_menus: menus copy shortcuts at call time.
            cx.bind_keys([
                KeyBinding::new("secondary-q", Quit, None),
                KeyBinding::new("secondary-shift-l", ToggleTheme, None),
                KeyBinding::new("secondary-n", app::NewPage, Some(app::WORKSPACE_CONTEXT)),
                KeyBinding::new("secondary-p", app::OpenSearch, Some(app::WORKSPACE_CONTEXT)),
                KeyBinding::new("secondary-k", app::OpenSearch, Some(app::WORKSPACE_CONTEXT)),
                KeyBinding::new(
                    "secondary-\\",
                    app::ToggleSidebar,
                    Some(app::WORKSPACE_CONTEXT),
                ),
                KeyBinding::new("secondary-[", app::GoBack, Some(app::WORKSPACE_CONTEXT)),
                KeyBinding::new("secondary-]", app::GoForward, Some(app::WORKSPACE_CONTEXT)),
            ]);
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.on_action(|_: &ToggleTheme, cx| {
                let mode = if cx.theme().is_dark() {
                    "light"
                } else {
                    "dark"
                };
                app::set_theme(mode, None, cx);
            });
            cx.set_menus(vec![
                Menu {
                    name: "Redbook".into(),
                    items: vec![MenuItem::action("Quit", Quit)],
                    disabled: false,
                },
                Menu {
                    name: "File".into(),
                    items: vec![
                        MenuItem::action("New Page", app::NewPage),
                        MenuItem::action("Search", app::OpenSearch),
                    ],
                    disabled: false,
                },
                Menu {
                    name: "View".into(),
                    items: vec![
                        MenuItem::action("Toggle Sidebar", app::ToggleSidebar),
                        MenuItem::action("Toggle Theme", ToggleTheme),
                    ],
                    disabled: false,
                },
                Menu {
                    name: "Go".into(),
                    items: vec![
                        MenuItem::action("Back", app::GoBack),
                        MenuItem::action("Forward", app::GoForward),
                    ],
                    disabled: false,
                },
            ]);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1200.), px(800.)), cx)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Redbook".into()),
                    ..Default::default()
                }),
                // Linux: matches the .desktop file so the dock groups the window.
                app_id: Some("redbook".into()),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                app::apply_theme(Some(window), cx);
                cx.new(|cx| app::AppView::new(window, cx))
            })
            .expect("failed to open window");
            cx.activate(true);
        });
}
