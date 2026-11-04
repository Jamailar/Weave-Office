// Release builds on Windows are GUI programs: double-clicking the app (or its Start menu shortcut)
// must not open a console window next to it. Debug builds keep the console for logs.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;
use weave_office::app::WeaveApp;

fn main() -> eframe::Result {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    // `weave-office <verb> …` (create, view, run, batch …) is the command line: no window.
    let console_output = weave_cli::is_cli(&args)
        || args.iter().any(|s| s == "--help" || s == "-h" || s == "--version");
    if console_output {
        attach_parent_console();
    }
    if weave_cli::is_cli(&args) {
        std::process::exit(weave_cli::main(&args));
    }
    if args.iter().any(|s| s == "--help" || s == "-h") {
        println!(
            "Weave Office\nUsage: weave-office [FILE...]\nOpen documents, spreadsheets, presentations and PDFs in one workspace.\n\nCommand line (no window): weave-office help"
        );
        return Ok(());
    }
    if args.iter().any(|s| s == "--version") {
        println!("Weave Office {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let startup_smoke = args.iter().any(|argument| argument == "--smoke-startup");
    let files: Vec<_> = args
        .into_iter()
        .filter(|argument| argument != "--smoke-startup")
        .map(PathBuf::from)
        .collect();
    if startup_smoke && !files.is_empty() {
        attach_parent_console();
        eprintln!("--smoke-startup cannot be combined with document paths");
        std::process::exit(2);
    }
    #[cfg(target_os = "macos")]
    let (_registration, inbox) = fmv_macos_events::Registration::install();
    let viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([1280.0, 860.0])
        .with_min_inner_size([800.0, 550.0])
        .with_app_id("local.weaveoffice.desktop");
    #[cfg(target_os = "macos")]
    let viewport = viewport
        // Keep AppKit's window frame and controls while drawing our file tabs
        // behind its transparent title bar. Native close still emits CloseRequested.
        .with_decorations(true)
        .with_fullsize_content_view(true)
        .with_titlebar_shown(false)
        .with_title_shown(false)
        .with_titlebar_buttons_shown(true)
        .with_has_shadow(true);
    #[cfg(not(target_os = "macos"))]
    let viewport = viewport.with_decorations(false);
    // Title bar, taskbar and Alt-Tab icon on Windows and Linux; the macOS bundle has its .icns.
    #[cfg(not(target_os = "macos"))]
    let viewport = match eframe::icon_data::from_png_bytes(include_bytes!("../assets/app-icon/weave-office-256.png")) {
        Ok(icon) => viewport.with_icon(icon),
        Err(_) => viewport,
    };
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        #[cfg(target_os = "macos")]
        event_loop_builder: Some(Box::new(|builder| {
            use winit::platform::macos::EventLoopBuilderExtMacOS;
            // Do not install an empty NSMenu: eframe's first app-icon update reads item 0.
            // Disable the default menu before startup so AppKit reports no menu at all.
            builder.with_default_menu(false);
        })),
        ..Default::default()
    };
    eframe::run_native(
        "Weave Office",
        options,
        Box::new(move |cc| {
            let mut app = WeaveApp::new(cc, files);
            if startup_smoke {
                app.startup_frames = Some(3);
            } else {
                app.enable_recovery();
            }
            #[cfg(target_os = "macos")]
            {
                let ctx = cc.egui_ctx.clone();
                inbox.set_wake(move || ctx.request_repaint());
                app.os_events = Some(inbox);
            }
            Ok(Box::new(app))
        }),
    )
}

/// A Windows GUI program starts without standard output. When it was started from a console with
/// nothing redirected (`weave-office help` typed into a terminal), write the command line's
/// output to that console. Redirected or piped output (scripts, AI agents) is inherited as is and
/// needs nothing. Elsewhere this does nothing.
fn attach_parent_console() {
    #[cfg(windows)]
    {
        use std::ffi::c_void;
        const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
        const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn AttachConsole(process_id: u32) -> i32;
            fn GetStdHandle(std_handle: u32) -> *mut c_void;
        }
        let missing = |handle: *mut c_void| handle.is_null() || handle as isize == -1;
        // SAFETY: plain Win32 calls with constant arguments; AttachConsole fails harmlessly when
        // there is no parent console. Only an unredirected launch attaches, so a redirected
        // stdout is never replaced.
        unsafe {
            if missing(GetStdHandle(STD_OUTPUT_HANDLE)) {
                AttachConsole(ATTACH_PARENT_PROCESS);
            }
        }
    }
}
