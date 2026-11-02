//! One egui font set for every open editor.
//!
//! Each editor installs the families it draws with (`ctx.set_fonts` replaces the whole set).
//! The host keeps what each tab installed and puts the union of all of them back into the
//! context, so switching tabs never changes fonts or rebuilds the glyph atlas: every family an
//! open editor needs is already there. Family names the editors share (`Proportional`,
//! `Monospace`, `medium`, ...) start with the same interface faces from [`crate::ui_fonts`];
//! the fallbacks behind them are joined in a fixed order (the shell first, then tabs in the
//! order they were opened), so the set does not depend on which tab is active.

use eframe::egui::{self, FontData, FontDefinitions};

/// The set the host keeps installed: the shell's faces plus every open tab's part.
///
/// egui keeps a font change queued during a pass in `Memory`, which the host saves and
/// restores per tab. A change an editor queued therefore lands while that same tab is
/// mounted, and whatever differs from the installed set when [`FontSet::capture`] runs belongs
/// to the mounted tab.
pub struct FontSet {
    shell: FontDefinitions,
    installed: Option<FontDefinitions>,
    /// The parts the installed set was merged from: tab ids in opening order.
    merged_from: Vec<weave_workspace::TabId>,
    /// The pass in which the installed set was queued rather than applied: egui applies it at
    /// the start of the next pass, so the context does not show it before then.
    queued_in: Option<u64>,
}

impl FontSet {
    pub fn new(shell: FontDefinitions) -> Self {
        Self {
            shell,
            installed: None,
            merged_from: Vec::new(),
            queued_in: None,
        }
    }

    /// Record a font change the mounted tab made into its `part`. Returns whether it changed.
    pub fn capture(
        &mut self,
        ctx: &egui::Context,
        part: Option<&mut Option<FontDefinitions>>,
    ) -> bool {
        // Before the first pass has ended the context may have no fonts yet; whatever the
        // first pass installs is seen at the next one.
        if ctx.cumulative_pass_nr() == 0 || self.queued_in == Some(ctx.cumulative_pass_nr()) {
            return false;
        }
        self.queued_in = None;
        let changed = ctx.fonts(|fonts| {
            self.installed
                .as_ref()
                .is_none_or(|installed| !same(installed, fonts.definitions()))
        });
        if !changed {
            return false;
        }
        match part {
            Some(part) => *part = Some(ctx.fonts(|fonts| fonts.definitions().clone())),
            None => {
                // Nothing is mounted: only the host itself changes fonts then.
                self.shell = ctx.fonts(|fonts| fonts.definitions().clone());
            }
        }
        // Forces a merge on the next `install`.
        self.installed = None;
        true
    }

    /// [`Self::capture`] for the `mounted` tab of `host`.
    pub fn capture_tab(
        &mut self,
        ctx: &egui::Context,
        host: &mut crate::host::Host,
        mounted: Option<weave_workspace::TabId>,
    ) -> bool {
        let part = mounted
            .and_then(|id| host.documents.get_mut(&id))
            .map(|doc| &mut doc.fonts);
        self.capture(ctx, part)
    }

    /// Keep the union of every open tab's fonts installed. Call at the start of each pass,
    /// before any text is laid out, and before changing the mounted tab.
    pub fn sync(
        &mut self,
        ctx: &egui::Context,
        host: &mut crate::host::Host,
        mounted: Option<weave_workspace::TabId>,
        interface: Option<&crate::ui_fonts::UiFonts>,
    ) {
        self.capture_tab(ctx, host, mounted);
        let parts = host
            .documents
            .iter()
            .filter_map(|(id, doc)| doc.fonts.as_ref().map(|fonts| (*id, fonts)));
        self.install(ctx, parts, interface);
    }

    /// Install the union of the shell and `parts` (each tab's part, keyed by its id), unless
    /// it is what the context has already. `interface` puts the host's interface faces first.
    pub fn install<'a>(
        &mut self,
        ctx: &egui::Context,
        parts: impl IntoIterator<Item = (weave_workspace::TabId, &'a FontDefinitions)>,
        interface: Option<&crate::ui_fonts::UiFonts>,
    ) {
        let mut parts: Vec<_> = parts.into_iter().collect();
        parts.sort_by_key(|(id, _)| *id);
        let ids: Vec<_> = parts.iter().map(|(id, _)| *id).collect();
        if self.installed.is_some() && ids == self.merged_from {
            return;
        }
        let mut set = merge(std::iter::once(&self.shell).chain(parts.iter().map(|(_, p)| *p)));
        if let Some(interface) = interface {
            interface.apply(&mut set);
        }
        if !ctx.fonts(|fonts| same(fonts.definitions(), &set)) {
            ctx.set_fonts(set.clone());
            if !ctx.fonts(|fonts| same(fonts.definitions(), &set)) {
                self.queued_in = Some(ctx.cumulative_pass_nr());
            }
        }
        self.installed = Some(set);
        self.merged_from = ids;
    }
}

/// The union of `parts`, earlier parts first. A face name present in several parts keeps the
/// first part's data, except that a part placing the same face on the baseline as a fallback wins
/// (see [`on_baseline_instead`]); a family present in several parts lists the first part's chain, then
/// the faces the later parts add, each face once.
pub fn merge<'a>(parts: impl IntoIterator<Item = &'a FontDefinitions>) -> FontDefinitions {
    let mut out = FontDefinitions {
        font_data: Default::default(),
        families: Default::default(),
    };
    for part in parts {
        for (name, data) in &part.font_data {
            let kept = out
                .font_data
                .entry(name.clone())
                .or_insert_with(|| data.clone());
            if on_baseline_instead(kept, data) {
                *kept = data.clone();
            }
        }
        for (family, chain) in &part.families {
            let merged = out.families.entry(family.clone()).or_default();
            for name in chain {
                if !merged.contains(name) {
                    merged.push(name.clone());
                }
            }
        }
    }
    // A family may only name faces the set has; `merge` of valid parts keeps that.
    debug_assert!(
        out.families
            .values()
            .flatten()
            .all(|n| out.font_data.contains_key(n))
    );
    out
}

/// Whether `later` is the same face as `kept` but put on the primary face's baseline when it is a
/// fallback, while `kept` centres it. Editors share face names (DeckCraft and GridCraft key the
/// craft-fonts faces alike); which tab was opened first must not decide whether Chinese next to
/// Latin in a spreadsheet cell sits on the Latin baseline. Centring only suits emoji and icon
/// faces, which no editor puts on the baseline, so the baseline wins regardless of order.
fn on_baseline_instead(kept: &FontData, later: &FontData) -> bool {
    use eframe::egui::epaint::text::FallbackAlign;
    kept.tweak.fallback_align != FallbackAlign::Baseline
        && later.tweak.fallback_align == FallbackAlign::Baseline
        && kept.index == later.index
        && (std::ptr::eq(kept.bytes(), later.bytes()) || kept.bytes() == later.bytes())
}

/// Whether two sets are the same, comparing face data by identity: a cheap per-frame check.
/// Face bytes that are equal but separately allocated count as different, which at worst
/// merges once more.
pub fn same(a: &FontDefinitions, b: &FontDefinitions) -> bool {
    a.families == b.families
        && a.font_data.len() == b.font_data.len()
        && a.font_data.iter().all(|(name, data)| {
            b.font_data
                .get(name)
                .is_some_and(|other| std::sync::Arc::ptr_eq(data, other))
        })
}

/// Whether `set` has everything `part` installed: its faces and, for each of its families,
/// all of that family's faces.
pub fn covers(set: &FontDefinitions, part: &FontDefinitions) -> bool {
    part.font_data
        .keys()
        .all(|name| set.font_data.contains_key(name))
        && part.families.iter().all(|(family, chain)| {
            set.families
                .get(family)
                .is_some_and(|have| chain.iter().all(|name| have.contains(name)))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{FontData, FontFamily};
    use std::sync::Arc;

    fn part(faces: &[&str], families: &[(&str, &[&str])]) -> FontDefinitions {
        let data = Arc::new(FontData::from_static(b"face"));
        FontDefinitions {
            font_data: faces
                .iter()
                .map(|f| ((*f).to_owned(), data.clone()))
                .collect(),
            families: families
                .iter()
                .map(|(family, chain)| {
                    let family = match *family {
                        "proportional" => FontFamily::Proportional,
                        name => FontFamily::Name(name.into()),
                    };
                    (family, chain.iter().map(|n| (*n).to_owned()).collect())
                })
                .collect(),
        }
    }

    #[test]
    fn a_shared_family_keeps_the_first_chain_and_adds_the_rest_once() {
        let word = part(
            &["ui", "w"],
            &[("proportional", &["ui", "w"]), ("doc", &["w"])],
        );
        let grid = part(
            &["ui", "g"],
            &[("proportional", &["ui", "g", "w"]), ("cell", &["g"])],
        );
        let set = merge([&word, &grid]);
        assert_eq!(set.families[&FontFamily::Proportional], ["ui", "w", "g"]);
        assert_eq!(set.families[&FontFamily::Name("doc".into())], ["w"]);
        assert_eq!(set.families[&FontFamily::Name("cell".into())], ["g"]);
        assert!(covers(&set, &word) && covers(&set, &grid));
        assert!(!covers(&word, &grid));
    }

    #[test]
    fn a_face_name_keeps_the_first_data() {
        let first = part(&["ui"], &[]);
        let mut second = part(&["ui"], &[]);
        second
            .font_data
            .insert("ui".into(), Arc::new(FontData::from_static(b"other")));
        let set = merge([&first, &second]);
        assert!(Arc::ptr_eq(&set.font_data["ui"], &first.font_data["ui"]));
    }

    #[test]
    fn a_face_on_the_baseline_wins_whatever_the_tab_order() {
        use eframe::egui::epaint::text::FallbackAlign;
        let centred = part(&["craft-han"], &[("deck", &["craft-han"])]);
        let mut baseline = part(&["craft-han"], &[("cell", &["craft-han"])]);
        let mut data = FontData::from_static(b"face");
        data.tweak.set_baseline_fallback(true);
        baseline
            .font_data
            .insert("craft-han".into(), Arc::new(data));
        for set in [merge([&centred, &baseline]), merge([&baseline, &centred])] {
            assert_eq!(
                set.font_data["craft-han"].tweak.fallback_align,
                FallbackAlign::Baseline
            );
            assert!(Arc::ptr_eq(
                &set.font_data["craft-han"],
                &baseline.font_data["craft-han"]
            ));
        }
        // A different face under the same name still keeps the first part's data.
        let mut other = FontData::from_static(b"other");
        other.tweak.set_baseline_fallback(true);
        let mut different = part(&["craft-han"], &[]);
        different
            .font_data
            .insert("craft-han".into(), Arc::new(other));
        let set = merge([&centred, &different]);
        assert!(Arc::ptr_eq(
            &set.font_data["craft-han"],
            &centred.font_data["craft-han"]
        ));
    }

    #[test]
    fn sameness_is_by_face_identity() {
        let a = part(&["ui"], &[("proportional", &["ui"])]);
        assert!(same(&a, &a.clone()));
        let b = part(&["ui"], &[("proportional", &["ui"])]);
        assert!(!same(&a, &b), "separately allocated data");
        let mut c = a.clone();
        c.families.insert(FontFamily::Monospace, vec!["ui".into()]);
        assert!(!same(&a, &c));
    }
}
