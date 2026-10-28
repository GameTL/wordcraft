//! Home › Font › Phonetic Guide: a table of base/ruby text (one row per Chinese character in
//! Mono layout, one row in Group layout), settings and a preview (#3384).

use super::chrome::buttons;
use super::{Dialog, Tokens, Ui, WordApp, json, vec2};

const ALIGNS: [(&str, &str); 6] = [
    ("center", "Center"),
    ("distributeLetter", "Distribute letters"),
    ("distributeSpace", "Distribute spacing"),
    ("left", "Left"),
    ("right", "Right"),
    ("rightVertical", "Right (vertical)"),
];

/// Rows the table shows before it scrolls.
const VISIBLE_ROWS: f32 = 8.0;
const COL_W: f32 = 150.0;
const PREVIEW_W: f32 = 400.0;
const PREVIEW_PAD: f32 = 8.0;
const BASE_PX: f32 = 24.0;
/// Most the Offset lifts the ruby in the preview, px.
const RAISE_MAX: f32 = 12.0;
const MAX_LINES: usize = 3;

type Units = [(String, String)];

/// The default readings, or the unit's own when it has one: whitespace inside a reading would
/// split it in two for the command, so it goes.
fn readings(units: &Units, original: &[String]) -> Vec<String> {
    units
        .iter()
        .enumerate()
        .map(|(i, (_, r))| {
            let r: String = r.split_whitespace().collect();
            if r.is_empty() { original.get(i).cloned().unwrap_or_default() } else { r }
        })
        .collect()
}

/// Mono → Group: the readings, one space apart.
fn to_group(units: &Units, original: &[String]) -> String {
    readings(units, original).join(" ")
}

/// Group → Mono: the typed words go to the characters when there are as many, else the default
/// readings come back.
fn to_mono(ruby: &str, units: &Units, original: &[String]) -> Vec<(String, String)> {
    let words: Vec<&str> = ruby.split_whitespace().collect();
    units
        .iter()
        .enumerate()
        .map(|(i, (b, _))| {
            let r = if words.len() == units.len() { words.get(i).map(|w| w.to_string()) } else { original.get(i).cloned() };
            (b.clone(), r.unwrap_or_default())
        })
        .collect()
}

/// What the preview and OK take: Mono, a (base, reading) per character; Group, the whole base
/// over the whole ruby.
fn preview_units(base: &str, ruby: &str, units: &Units, original: &[String], group: bool) -> Vec<(String, String)> {
    if group {
        return vec![(base.to_string(), ruby.trim().to_string())];
    }
    units.iter().map(|(b, _)| b.clone()).zip(readings(units, original)).collect()
}

/// Left edge of each unit and its line, wrapping at `max_w`; a unit wider than a line sits alone.
fn flow(widths: &[f32], max_w: f32, gap: f32) -> Vec<(usize, f32)> {
    let (mut line, mut x) = (0, 0.0_f32);
    widths
        .iter()
        .map(|&w| {
            if x > 0.0 && x + w > max_w {
                line += 1;
                x = 0.0;
            }
            let at = (line, x);
            x += w + gap;
            at
        })
        .collect()
}

/// Where the ruby starts inside a unit `unit_w` wide.
fn ruby_x(align: &str, unit_w: f32, ruby_w: f32) -> f32 {
    match align {
        "left" => 0.0,
        "right" | "rightVertical" => unit_w - ruby_w,
        _ => (unit_w - ruby_w) / 2.0,
    }
}

fn finite(x: f32, default: f32) -> f32 {
    if x.is_finite() { x } else { default }
}

/// The Preview box: ruby over base, left to right, wrapping, clipped to the box.
fn preview(ui: &mut Ui, items: &[(String, String)], align: &str, size: f32, offset: f32, base_size: f32) {
    let t = Tokens::get(ui.ctx());
    let ink = ui.visuals().text_color();
    let bs = finite(base_size, 10.5);
    let bs = if bs > 0.0 { bs } else { 10.5 };
    let ruby_px = (BASE_PX * finite(size, bs / 2.0).max(0.0) / bs).clamp(6.0, 24.0);
    let raise = (finite(offset, 0.0) * BASE_PX / bs).clamp(0.0, RAISE_MAX);
    let slot = ruby_px * 1.2;
    let line_h = RAISE_MAX + slot + BASE_PX * 1.2;
    let galley = |text: &str, px: f32| ui.painter().layout_no_wrap(text.to_string(), egui::FontId::proportional(px), ink);
    let cells: Vec<_> = items.iter().map(|(b, r)| (galley(b, BASE_PX), galley(r, ruby_px))).collect();
    let widths: Vec<f32> = cells.iter().map(|(b, r)| b.size().x.max(r.size().x)).collect();
    let places = flow(&widths, PREVIEW_W - 2.0 * PREVIEW_PAD, 4.0);
    let lines = places.last().map_or(1, |(l, _)| l + 1).min(MAX_LINES);
    let (rect, _) = ui.allocate_exact_size(vec2(PREVIEW_W, lines as f32 * line_h + 2.0 * PREVIEW_PAD), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 2.0, t.input);
    p.rect_stroke(rect, 2.0, egui::Stroke::new(1.0, t.border_strong), egui::StrokeKind::Inside);
    for (((b, r), w), (line, x)) in cells.into_iter().zip(widths).zip(places) {
        let left = rect.left() + PREVIEW_PAD + x;
        let top = rect.top() + PREVIEW_PAD + line as f32 * line_h;
        let ruby_at = egui::pos2(left + ruby_x(align, w, r.size().x), top + RAISE_MAX - raise);
        let base_at = egui::pos2(left + (w - b.size().x) / 2.0, top + RAISE_MAX + slot);
        p.galley(ruby_at, r, ink);
        p.galley(base_at, b, ink);
    }
}

/// Returns true to close.
pub(super) fn phonetic_dialog(app: &mut WordApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let Dialog::Phonetic { base, ruby, units, original, group, base_size, align, size, offset, existing, focus } = d else {
        return false;
    };
    // Base and ruby text are usually Chinese or Japanese: fonts for them load from the next frame.
    app.want_system_cjk = true;
    let strong = |s: &str| egui::RichText::new(s.to_string()).strong();
    let (base_head, ruby_head) = (tl!("Base text:"), tl!("Ruby text:"));
    let ruby_label = egui::Grid::new("phonetic_head")
        .num_columns(2)
        .min_col_width(COL_W)
        .show(ui, |ui| {
            ui.label(strong(base_head));
            let l = ui.label(strong(ruby_head));
            ui.end_row();
            l
        })
        .inner;
    egui::ScrollArea::vertical().id_salt("phonetic_rows").max_height(VISIBLE_ROWS * 26.0).auto_shrink([false, true]).show(ui, |ui| {
        egui::Grid::new("phonetic_rows").num_columns(2).min_col_width(COL_W).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
            let mut first = true;
            let mut edit = |ui: &mut Ui, text: &mut String, focus: &mut bool| {
                let r = ui.add(egui::TextEdit::singleline(text).desired_width(COL_W));
                // Typing goes to the first Ruby text box, not the selected text in the document (#3914).
                if first && std::mem::take(focus) {
                    r.request_focus();
                }
                first = false;
                r
            };
            if *group {
                ui.scope(|ui| {
                    ui.set_max_width(COL_W);
                    ui.add(egui::Label::new(egui::RichText::new(base.as_str())).truncate());
                });
                edit(ui, ruby, focus).labelled_by(ruby_label.id);
                ui.end_row();
            } else {
                for (b, r) in units.iter_mut() {
                    ui.label(b.as_str());
                    let name = crate::i18n::fmt(tl!("Ruby text for {base}"), &[("base", b)]);
                    let resp = edit(ui, r, focus);
                    ui.ctx().accesskit_node_builder(resp.id, |node| node.set_label(name));
                    ui.end_row();
                }
            }
        });
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui.add_enabled(*group && !units.is_empty(), egui::Button::new(tl!("Mono Ruby"))).clicked() {
            *units = to_mono(ruby, units, original);
            *group = false;
        }
        if ui.add_enabled(!*group, egui::Button::new(tl!("Group Ruby"))).clicked() {
            *ruby = to_group(units, original);
            *group = true;
        }
    });
    ui.add_space(4.0);
    egui::Grid::new("phonetic").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
        let align_label = ui.label(tl!("Alignment:"));
        let shown = ALIGNS.iter().find(|(k, _)| k == align).map_or("Center", |(_, l)| l);
        let combo = craft_ui::ComboBox::from_id_salt("phonetic_align").selected_text(tl!(shown)).show_ui(ui, |ui| {
            for (k, l) in ALIGNS {
                ui.selectable_value(align, k.to_string(), tl!(l));
            }
        });
        combo.response.labelled_by(align_label.id);
        ui.end_row();
        let offset_label = ui.label(tl!("Offset (pt):"));
        ui.add(egui::DragValue::new(offset).speed(0.5).range(-100.0..=100.0)).labelled_by(offset_label.id);
        ui.end_row();
        let size_label = ui.label(tl!("Size (pt):"));
        ui.add(egui::DragValue::new(size).speed(0.5).range(1.0..=200.0)).labelled_by(size_label.id);
        ui.end_row();
    });
    ui.add_space(4.0);
    ui.label(strong(tl!("Preview")));
    let items = preview_units(base, ruby, units, original, *group);
    preview(ui, &items, align, *size, *offset, *base_size);
    let remove = *existing && ui.button(tl!("Remove")).clicked();
    let (ok, cancel) = buttons(ui, tl!("OK"));
    if remove {
        let _ = app.run("format.phonetic", json!({"ruby": null}));
    }
    // A failed call (a reading count that doesn't match the characters…) keeps the dialog and the typed readings.
    let mut applied = false;
    if ok && !remove {
        applied = if *group {
            app.run("format.phonetic", json!({"ruby": ruby, "align": align, "size": *size, "offset": *offset})).is_ok()
        } else {
            // ponytail: an emptied box takes its character's default reading, so the counts always
            // match and a single character can't be left without a guide here.
            let ruby = readings(units, original).join(" ");
            app.run("format.phonetic", json!({"mono": true, "ruby": ruby, "align": align, "size": *size, "offset": *offset})).is_ok()
        };
    }
    applied || cancel || remove
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter().map(|(b, r)| (b.to_string(), r.to_string())).collect()
    }

    #[test]
    fn switching_layouts_keeps_or_restores_the_readings() {
        let original = vec!["yín".to_string(), "xíng".to_string()];
        let units = u(&[("银", "yín"), ("行", "háng")]);
        assert_eq!(to_group(&units, &original), "yín háng");
        assert_eq!(to_mono("yín háng", &units, &original), units);
        // Wrong count: the default readings come back.
        assert_eq!(to_mono("yín", &units, &original), u(&[("银", "yín"), ("行", "xíng")]));
        assert_eq!(to_mono("", &units, &original), u(&[("银", "yín"), ("行", "xíng")]));
    }

    #[test]
    fn an_empty_reading_falls_back_to_the_default_one() {
        let original = vec!["yín".to_string(), "xíng".to_string()];
        let units = u(&[("银", " "), ("行", "há ng")]);
        assert_eq!(readings(&units, &original), ["yín", "háng"]);
        assert_eq!(preview_units("银行", "x", &units, &original, false), u(&[("银", "yín"), ("行", "háng")]));
        assert_eq!(preview_units("银行", " x ", &units, &original, true), u(&[("银行", "x")]));
        assert!(preview_units("", "", &[], &[], false).is_empty());
    }

    #[test]
    fn units_wrap_at_the_box_width() {
        assert_eq!(flow(&[30.0, 30.0, 30.0], 70.0, 4.0), [(0, 0.0), (0, 34.0), (1, 0.0)]);
        assert_eq!(flow(&[500.0, 10.0], 100.0, 4.0), [(0, 0.0), (1, 0.0)], "a wide unit sits alone");
        assert_eq!((ruby_x("left", 40.0, 10.0), ruby_x("rightVertical", 40.0, 10.0), ruby_x("distributeSpace", 40.0, 10.0)), (0.0, 30.0, 15.0));
    }
}
