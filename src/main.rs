use std::path::PathBuf;
use weave_office::app::WeaveApp;

fn main() -> eframe::Result {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    // `weave-office <verb> …` (create, view, run, batch …) is the command line: no window.
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
