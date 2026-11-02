use super::*;

#[test]
fn every_tab_fits_when_there_is_room() {
    assert_eq!(fit(&[50.0, 60.0, 70.0], 200.0, Some(2)), [true, true, true]);
}

#[test]
fn the_leading_tabs_fit_and_leave_room_for_the_overflow_button() {
    // 50 + 60 + gaps = 114; room is 150 - 26 - 2 = 122, so the third (70) does not fit.
    assert_eq!(
        fit(&[50.0, 60.0, 70.0, 40.0], 150.0, Some(0)),
        [true, true, false, false]
    );
}

#[test]
fn the_open_tab_is_always_shown() {
    assert_eq!(
        fit(&[50.0, 60.0, 70.0, 40.0], 150.0, Some(3)),
        [true, false, false, true]
    );
    assert_eq!(
        fit(&[50.0, 60.0, 70.0, 40.0], 120.0, Some(2)),
        [false, false, true, false]
    );
}

#[test]
fn the_row_reports_each_shown_tab_and_a_click() {
    let ctx = egui::Context::default();
    let tabs = Tabs {
        tabs: vec![
            Tab::new("Home", "开始"),
            Tab::new("Insert", "插入"),
            Tab::new("Table", "表设计").contextual(true),
        ],
        active: Some("Home".into()),
    };
    let mut clicked_at = None;
    for frame in 0..3 {
        let mut input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 40.0))),
            time: Some(frame as f64 / 60.0),
            ..Default::default()
        };
        if let Some(at) = clicked_at {
            input.events = vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ];
        }
        let mut response = Response::default();
        ctx.run_ui(input, |ui| {
            ui.horizontal(|ui| response = strip(ui, &tabs));
        })
        .drop_without_applying_deltas();
        assert_eq!(response.rects.len(), 3);
        if frame == 1 {
            assert_eq!(response.event, Some(Event::Clicked("Insert".into())));
        }
        clicked_at = Some(response.rects[1].1.center());
    }
}
