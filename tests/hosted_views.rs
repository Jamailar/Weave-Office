//! Render the real embedded views, with native accessibility enabled, without opening windows.
use eframe::egui;
use weave_office::host::Host;
use weave_workspace::{DocumentKind, TabId};

fn raw(frame: u64) -> egui::RawInput {
    egui::RawInput {
        focused: true,
        time: Some(frame as f64 / 60.0),
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 860.0),
        )),
        ..Default::default()
    }
}

/// Install a tab's retained UI state, as required by each editor's hosted contract.
fn mount(ctx: &egui::Context, host: &mut Host, mounted: &mut Option<TabId>, next: TabId) {
    if let Some(old) = *mounted {
        let document = host.documents.get_mut(&old).expect("previous view");
        document
            .editor
            .deactivate(ctx)
            .expect("leave previous view");
        document.memory = ctx.memory(Clone::clone);
        document.fonts = Some(ctx.fonts(|fonts| fonts.definitions().clone()));
    }
    let options = ctx.options(Clone::clone);
    let document = host.documents.get(&next).expect("next view");
    ctx.memory_mut(|memory| {
        *memory = document.memory.clone();
        memory.options = options;
    });
    ctx.set_fonts(document.fonts.clone().unwrap_or_default());
    assert!(host.workspace.activate(next));
    *mounted = Some(next);
}

fn render(ctx: &egui::Context, host: &mut Host, frame: u64) -> usize {
    let document = host.active_mut().expect("active view");
    let mut input = raw(frame);
    document.editor.raw_input(ctx, &mut input);
    let output = ctx.run_ui(input, |ui| {
        document.editor.logic(ctx);
        // The host lays out its own text before the editing surface. This also exercises
        // deferred font installation and a second egui pass, rather than an isolated demo UI.
        egui::Panel::top("hosted-view-test-tabs").show(ui, |ui| {
            ui.label("Weave Office 文件标签");
        });
        document.editor.ui(ui);
    });
    let nodes = output
        .platform_output
        .accesskit_update
        .as_ref()
        .map_or(0, |tree| tree.nodes.len());
    let _ = ctx.tessellate(output.shapes.clone(), output.pixels_per_point);
    output.drop_without_applying_deltas();
    nodes
}

#[test]
fn every_hosted_view_renders_with_accessibility_after_rapid_mixed_tab_switches() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    ctx.run_ui(raw(0), |_ui| {}).drop_without_applying_deltas();
    let mut host = Host::default();
    let kinds = [
        DocumentKind::Text,
        DocumentKind::Spreadsheet,
        DocumentKind::Presentation,
        DocumentKind::Pdf,
    ];
    let ids: Vec<_> = kinds
        .into_iter()
        .chain(kinds)
        .map(|kind| host.new_document(kind).expect("create view"))
        .collect();
    let mut mounted = None;
    let mut frame = 1;
    // A batch open may visit a file for just one frame before processing the next file.
    for &id in &ids {
        mount(&ctx, &mut host, &mut mounted, id);
        render(&ctx, &mut host, frame);
        frame += 1;
    }
    for &id in ids.iter().rev() {
        mount(&ctx, &mut host, &mut mounted, id);
        let mut nodes = 0;
        for _ in 0..4 {
            nodes = render(&ctx, &mut host, frame);
            frame += 1;
        }
        assert!(
            nodes > 10,
            "the actual editing view must produce accessible controls for {id:?}, got {nodes}"
        );
    }
    assert_eq!(host.workspace.tabs().len(), 8);
}
