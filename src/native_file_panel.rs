//! Keep application quit reachable without a key window, and supply native-panel shortcuts.

use eframe::egui;

/// Retain the application shortcut target and restore the previous menu on shutdown.
#[derive(Default)]
#[must_use = "Keep the application menu alive for the lifetime of the host"]
pub struct AppMenu {
    #[cfg(target_os = "macos")]
    _guard: Option<macos::MenuGuard>,
}

impl AppMenu {
    /// Update the retained application's labels after the host publishes its language.
    /// Does not replace the active menu or disturb a temporary native file-panel menu.
    pub fn refresh_language(&self) {
        #[cfg(target_os = "macos")]
        if let Some(guard) = &self._guard {
            guard.refresh_language();
        }
    }
}

pub fn install_app_menu(ctx: &egui::Context) -> AppMenu {
    #[cfg(target_os = "macos")]
    {
        AppMenu {
            _guard: macos::install(ctx, true),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = ctx;
        AppMenu::default()
    }
}

pub(crate) fn reveal_app() {
    #[cfg(target_os = "macos")]
    macos::reveal_app();
}

pub fn with_file_panel_menu<T>(ctx: &egui::Context, operation: impl FnOnce() -> T) -> T {
    #[cfg(target_os = "macos")]
    let _menu = macos::install(ctx, false);
    #[cfg(not(target_os = "macos"))]
    let _ = ctx;
    operation()
}

#[cfg(target_os = "macos")]
mod macos {
    use super::egui;
    use objc2::{
        DefinedClass, MainThreadOnly, define_class, msg_send, rc::Retained, runtime::AnyObject, sel,
    };
    use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
    use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol, NSString, ns_string};

    define_class!(
        // SAFETY: NSObject has no subclass invariants; this target and its
        // retained egui context are only accessed from the AppKit main thread.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[ivars = egui::Context]
        struct WeaveFilePanelQuitTarget;

        unsafe impl NSObjectProtocol for WeaveFilePanelQuitTarget {}

        impl WeaveFilePanelQuitTarget {
            // AppKit requires this on a menu target during a modal session:
            // https://developer.apple.com/documentation/appkit/nsmenuitem/target
            #[unsafe(method(worksWhenModal))]
            fn works_when_modal(&self) -> bool {
                true
            }

            // SAFETY: Standard target/action signature: optional sender, no result.
            #[unsafe(method(requestQuit:))]
            fn request_quit(&self, _sender: Option<&AnyObject>) {
                // AppKit can dispatch the application shortcut with no key window. Restore
                // only this application's windows so its unsaved-work prompt can be shown.
                reveal_app();
                let app = NSApplication::sharedApplication(self.mtm());
                self.ivars().send_viewport_cmd_to(
                    egui::ViewportId::ROOT,
                    egui::ViewportCommand::Close,
                );
                self.ivars().request_repaint();
                if app.modalWindow().is_some() {
                    app.abortModal();
                }
            }
        }
    );

    pub(super) fn reveal_app() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        app.unhide(None);
        for window in app.windows().iter() {
            if window.isMiniaturized() {
                window.deminiaturize(None);
            }
        }
    }

    fn append_submenu(menu: &NSMenu, title: &NSString, submenu: &NSMenu, mtm: MainThreadMarker) {
        let item = NSMenuItem::new(mtm);
        item.setTitle(title);
        submenu.setTitle(title);
        item.setSubmenu(Some(submenu));
        menu.addItem(&item);
    }

    pub(super) struct MenuGuard {
        app: Retained<NSApplication>,
        previous_menu: Option<Retained<NSMenu>>,
        _menu: Retained<NSMenu>,
        quit: Retained<NSMenuItem>,
        // NSMenuItem's target is weak: retain it until the old menu is restored.
        _quit_target: Retained<WeaveFilePanelQuitTarget>,
    }

    impl MenuGuard {
        pub(super) fn refresh_language(&self) {
            self.quit.setTitle(&NSString::from_str(crate::language::tr(
                "退出 Weave Office",
            )));
        }
    }

    impl Drop for MenuGuard {
        fn drop(&mut self) {
            self.app.setMainMenu(self.previous_menu.as_deref());
        }
    }

    pub(super) fn install(ctx: &egui::Context, application_only: bool) -> Option<MenuGuard> {
        let mtm = MainThreadMarker::new()?;
        let allocated = WeaveFilePanelQuitTarget::alloc(mtm).set_ivars(ctx.clone());
        // SAFETY: NSObject's init completes initialization of our allocated subclass.
        let quit_target: Retained<WeaveFilePanelQuitTarget> =
            unsafe { msg_send![super(allocated), init] };

        let menu = NSMenu::new(mtm);
        let app_menu = NSMenu::new(mtm);
        // SAFETY: requestQuit: is implemented above with AppKit's action signature.
        let quit = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(crate::language::tr("退出 Weave Office")),
                Some(sel!(requestQuit:)),
                ns_string!("q"),
            )
        };
        // SAFETY: The target implements this item's action and the guard keeps
        // it alive until the temporary menu is no longer installed.
        unsafe { quit.setTarget(Some(&quit_target)) };
        quit.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
        // Visible application commands belong to Weave's own chrome. The native item only
        // supplies AppKit's application-level accelerator, including without a key window.
        quit.setHidden(application_only);
        quit.setAllowsKeyEquivalentWhenHidden(application_only);
        app_menu.addItem(&quit);
        append_submenu(&menu, ns_string!("Weave Office"), &app_menu, mtm);

        if !application_only {
            let edit_menu = NSMenu::new(mtm);
            for (title, action, key) in [
                ("剪切", sel!(cut:), ns_string!("x")),
                ("复制", sel!(copy:), ns_string!("c")),
                ("粘贴", sel!(paste:), ns_string!("v")),
                ("全选", sel!(selectAll:), ns_string!("a")),
            ] {
                // SAFETY: These are AppKit's standard one-sender responder actions.
                // A nil target leaves validation and dispatch to the native field.
                let item = unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        NSMenuItem::alloc(mtm),
                        &NSString::from_str(crate::language::tr(title)),
                        Some(action),
                        key,
                    )
                };
                item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
                edit_menu.addItem(&item);
            }
            append_submenu(
                &menu,
                &NSString::from_str(crate::language::tr("编辑")),
                &edit_menu,
                mtm,
            );
        }

        let app = NSApplication::sharedApplication(mtm);
        let guard = MenuGuard {
            previous_menu: app.mainMenu(),
            app,
            _menu: menu,
            quit,
            _quit_target: quit_target,
        };
        // The menu already has an application submenu: eframe may safely read
        // its first item. The guard also restores state if operation unwinds.
        guard.app.setMainMenu(Some(&guard._menu));
        Some(guard)
    }
}
