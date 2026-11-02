//! Run with `harness = false`: AppKit must execute on the process's actual main thread.
//! This process creates no windows and sends no events to another application.

#[cfg(target_os = "macos")]
fn main() {
    objc2::rc::autoreleasepool(|_| macos::verify_application_menu());
}

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
mod macos {
    use eframe::egui;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSApplication, NSEvent, NSEventModifierFlags, NSEventType, NSMenu};
    use objc2_foundation::{MainThreadMarker, NSPoint, NSString};
    use weave_office::language::{self, UiLanguage};
    use weave_office::native_file_panel::{install_app_menu, with_file_panel_menu};

    fn key_event(key: &str, modifiers: NSEventModifierFlags) -> Retained<NSEvent> {
        let characters = NSString::from_str(key);
        NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            NSEventType::KeyDown,
            NSPoint::new(0.0, 0.0),
            modifiers,
            0.0,
            0,
            None,
            &characters,
            &characters,
            false,
            if key == "q" { 12 } else { 13 },
        )
        .expect("construct an application-level key event without a window")
    }

    fn take_close_count(ctx: &egui::Context) -> usize {
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                ..Default::default()
            },
            |_| {},
        );
        let count = output
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map_or(0, |viewport| {
                viewport
                    .commands
                    .iter()
                    .filter(|command| matches!(command, egui::ViewportCommand::Close))
                    .count()
            });
        output.drop_without_applying_deltas();
        count
    }

    fn assert_same_menu(actual: &NSMenu, expected: &NSMenu) {
        assert!(
            std::ptr::eq(actual, expected),
            "restore the same retained menu, not a replacement"
        );
    }

    pub fn verify_application_menu() {
        let mtm = MainThreadMarker::new().expect("AppKit test must run on the process main thread");
        let app = NSApplication::sharedApplication(mtm);
        assert!(app.keyWindow().is_none());
        assert_eq!(
            app.windows().len(),
            0,
            "this test must not create native windows"
        );
        let previous_menu = app.mainMenu();
        let ctx = egui::Context::default();
        assert_eq!(take_close_count(&ctx), 0);
        language::set_current(UiLanguage::ZhHans);
        let lifetime = install_app_menu(&ctx);
        let permanent = app.mainMenu().expect("application menu installed");
        assert_eq!(permanent.numberOfItems(), 1);
        let submenu = permanent
            .itemAtIndex(0)
            .unwrap()
            .submenu()
            .expect("safe for eframe's item-0 lookup");
        assert_eq!(submenu.numberOfItems(), 1);
        let quit = submenu.itemAtIndex(0).unwrap();
        assert!(quit.isHidden());
        assert!(quit.allowsKeyEquivalentWhenHidden());
        assert_eq!(quit.title().to_string(), "退出 Weave Office");
        language::set_current(UiLanguage::En);
        lifetime.refresh_language();
        assert_same_menu(&app.mainMenu().unwrap(), &permanent);
        assert!(std::ptr::eq(&*submenu.itemAtIndex(0).unwrap(), &*quit));
        assert_eq!(quit.title().to_string(), "Quit Weave Office");
        assert!(quit.isHidden());
        assert!(quit.allowsKeyEquivalentWhenHidden());
        assert_eq!(
            take_close_count(&ctx),
            0,
            "translating must not dispatch Quit"
        );

        // Exercise actual AppKit accelerator lookup, not merely the item's properties.
        let cmd_q = key_event("q", NSEventModifierFlags::Command);
        assert!(permanent.performKeyEquivalent(&cmd_q));
        assert_eq!(
            take_close_count(&ctx),
            1,
            "one shortcut queues one host close request"
        );
        assert_eq!(
            take_close_count(&ctx),
            0,
            "the request is not replayed on the next frame"
        );
        assert!(!permanent.performKeyEquivalent(&key_event("q", NSEventModifierFlags::empty())));
        assert!(!permanent.performKeyEquivalent(&key_event("w", NSEventModifierFlags::Command)));
        assert_eq!(
            take_close_count(&ctx),
            0,
            "unrelated keys stay available to the normal responder"
        );

        with_file_panel_menu(&ctx, || {
            let temporary = app.mainMenu().expect("file-panel menu installed");
            assert!(!std::ptr::eq(&*temporary, &*permanent));
            assert_eq!(
                temporary.numberOfItems(),
                2,
                "native panels retain Quit and Edit menus"
            );
            let quit = temporary
                .itemAtIndex(0)
                .unwrap()
                .submenu()
                .unwrap()
                .itemAtIndex(0)
                .unwrap();
            assert!(!quit.isHidden());
            assert_eq!(quit.title().to_string(), "Quit Weave Office");
            let edit = temporary.itemAtIndex(1).unwrap().submenu().unwrap();
            assert_eq!(
                edit.numberOfItems(),
                4,
                "native-field Cut/Copy/Paste/Select All remain present"
            );
            // Refresh the retained permanent item while a temporary menu is active. It must
            // neither replace that menu nor change the guard's later restoration target.
            language::set_current(UiLanguage::ZhHans);
            lifetime.refresh_language();
            assert_same_menu(&app.mainMenu().unwrap(), &temporary);
            assert_eq!(
                submenu.itemAtIndex(0).unwrap().title().to_string(),
                "退出 Weave Office"
            );
            assert_eq!(quit.title().to_string(), "Quit Weave Office");
            assert!(temporary.performKeyEquivalent(&cmd_q));
            assert_eq!(take_close_count(&ctx), 1);
        });
        assert_same_menu(&app.mainMenu().unwrap(), &permanent);
        assert!(
            permanent.performKeyEquivalent(&cmd_q),
            "permanent weak target remains alive after the panel scope"
        );
        assert_eq!(take_close_count(&ctx), 1);
        assert!(app.keyWindow().is_none());
        assert_eq!(app.windows().len(), 0);

        drop(lifetime);
        match (app.mainMenu(), previous_menu) {
            (Some(actual), Some(expected)) => assert_same_menu(&actual, &expected),
            (None, None) => {}
            _ => panic!("application menu lifetime did not restore the prior menu"),
        }
        println!(
            "native application menu: in-place language refresh, hidden Cmd-Q, single dispatch, no-key-window and panel restoration passed"
        );
    }
}
