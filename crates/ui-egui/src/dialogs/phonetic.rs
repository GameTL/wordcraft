//! Home › Font › Phonetic Guide: a row of character cells (reading above, base below), as in
//! Word, then Alignment / Offset / Size and a preview (#3384).

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

const CELL_W: f32 = 76.0;
const PREVIEW_W: f32 = 440.0;
const PREVIEW_PAD: f32 = 10.0;
const BASE_PX: f32 = 22.0;
/// Most the Offset lifts the ruby in the preview, px.
const RAISE_MAX: f32 = 12.0;

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

/// What the preview draws: Mono, a (base, reading) per character; Group, the whole base over the
/// joined reading, still one cell per character for the bases.
fn preview_items(units: &Units, original: &[String], group: bool) -> (Vec<(String, String)>, Option<String>) {
    let items: Vec<(String, String)> = units.iter().map(|(b, _)| b.clone()).zip(readings(units, original)).collect();
    let joined = group.then(|| readings(units, original).join(" "));
    (items, joined)
}

/// Where the ruby starts inside a unit `unit_w` wide.
fn ruby_x(align: &str, unit_w: f32, ruby_w: f32) -> f32 {
    match align {
        "left" => 0.0,
        "right" | "rightVertical" => (unit_w - ruby_w).max(0.0),
        _ => ((unit_w - ruby_w) / 2.0).max(0.0),
    }
}

fn finite(x: f32, default: f32) -> f32 {
    if x.is_finite() { x } else { default }
}

/// The Preview box: ruby over each base, or one reading over the whole row when grouped.
fn preview(ui: &mut Ui, items: &[(String, String)], joined: Option<&str>, align: &str, size: f32, offset: f32, base_size: f32) {
    let t = Tokens::get(ui.ctx());
    let ink = ui.visuals().text_color();
    let bs = finite(base_size, 10.5);
    let bs = if bs > 0.0 { bs } else { 10.5 };
    let ruby_px = (BASE_PX * finite(size, bs / 2.0).max(0.0) / bs).clamp(8.0, 22.0);
    let raise = (finite(offset, 0.0) * BASE_PX / bs).clamp(0.0, RAISE_MAX);
    let slot = ruby_px * 1.25;
    let line_h = RAISE_MAX + slot + BASE_PX * 1.3;
    let galley = |text: &str, px: f32| ui.painter().layout_no_wrap(text.to_string(), egui::FontId::proportional(px), ink);
    let bases: Vec<_> = items.iter().map(|(b, _)| galley(b, BASE_PX)).collect();
    let rubies: Vec<_> = items.iter().map(|(_, r)| galley(r, ruby_px)).collect();
    let joined_g = joined.filter(|s| !s.is_empty()).map(|s| galley(s, ruby_px));
    let n = items.len().max(1) as f32;
    let inner_w = PREVIEW_W - 2.0 * PREVIEW_PAD;
    let cell = (inner_w / n).clamp(28.0, CELL_W);
    let row_w = cell * n;
    let (rect, _) = ui.allocate_exact_size(vec2(PREVIEW_W, line_h + 2.0 * PREVIEW_PAD), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 2.0, t.input);
    p.rect_stroke(rect, 2.0, egui::Stroke::new(1.0, t.border_strong), egui::StrokeKind::Inside);
    let origin = egui::pos2(rect.left() + PREVIEW_PAD + ((inner_w - row_w) / 2.0).max(0.0), rect.top() + PREVIEW_PAD);
    if let Some(g) = joined_g {
        let x = origin.x + ruby_x(align, row_w, g.size().x);
        p.galley(egui::pos2(x, origin.y + RAISE_MAX - raise), g, ink);
        for (i, b) in bases.into_iter().enumerate() {
            let left = origin.x + i as f32 * cell;
            p.galley(egui::pos2(left + (cell - b.size().x) / 2.0, origin.y + RAISE_MAX + slot), b, ink);
        }
        return;
    }
    for (i, (b, r)) in bases.into_iter().zip(rubies).enumerate() {
        let left = origin.x + i as f32 * cell;
        p.galley(egui::pos2(left + ruby_x(align, cell, r.size().x), origin.y + RAISE_MAX - raise), r, ink);
        p.galley(egui::pos2(left + (cell - b.size().x) / 2.0, origin.y + RAISE_MAX + slot), b, ink);
    }
}

/// Returns true to close.
pub(super) fn phonetic_dialog(app: &mut WordApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let Dialog::Phonetic { base, ruby, units, original, group, base_size, align, size, offset, existing, focus } = d else {
        return false;
    };
    // Base and ruby text are usually Chinese or Japanese: fonts for them load from the next frame.
    let mut font_text = base.clone();
    for (b, _) in units.iter() {
        font_text.push_str(b);
    }
    app.ensure_phonetic_fonts(&font_text);
    ui.set_min_width(PREVIEW_W);
    let cells = !units.is_empty();
    if cells {
        // Word: one column per character, reading on top, base underneath.
        ui.group(|ui| {
            egui::ScrollArea::horizontal().id_salt("phonetic_cells").max_width(PREVIEW_W).auto_shrink([false, true]).show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                ui.horizontal(|ui| {
                    let mut first = true;
                    for (b, r) in units.iter_mut() {
                        egui::Frame::group(ui.style()).inner_margin(6.0).show(ui, |ui| {
                            ui.set_width(CELL_W);
                            ui.vertical(|ui| {
                                let resp = ui.add(egui::TextEdit::singleline(r).desired_width(CELL_W).align(egui::Align2::CENTER_CENTER));
                                let name = crate::i18n::fmt(tl!("Ruby text for {base}"), &[("base", b)]);
                                ui.ctx().accesskit_node_builder(resp.id, |node| node.set_label(name));
                                if first && std::mem::take(focus) {
                                    resp.request_focus();
                                }
                                first = false;
                                ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                                    ui.label(egui::RichText::new(b.as_str()).size(20.0).strong());
                                });
                            });
                        });
                    }
                });
            });
        });
    } else {
        let ruby_label = ui.label(tl!("Ruby text:"));
        let r = ui.add(egui::TextEdit::singleline(ruby).desired_width(PREVIEW_W - 80.0)).labelled_by(ruby_label.id);
        if std::mem::take(focus) {
            r.request_focus();
        }
        if !base.is_empty() {
            ui.horizontal(|ui| {
                ui.label(tl!("Base text:"));
                ui.label(egui::RichText::new(base.as_str()).strong());
            });
        }
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.add_enabled(cells && *group, egui::Button::new(tl!("Mono Ruby"))).clicked() {
            *group = false;
        }
        if ui.add_enabled(cells && !*group, egui::Button::new(tl!("Group Ruby"))).clicked() {
            *ruby = readings(units, original).join(" ");
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
    ui.label(egui::RichText::new(tl!("Preview")).strong());
    let (items, joined) = if cells { preview_items(units, original, *group) } else { (vec![(base.clone(), ruby.clone())], None) };
    preview(ui, &items, joined.as_deref(), align, *size, *offset, *base_size);
    let remove = *existing && ui.button(tl!("Remove")).clicked();
    let (ok, cancel) = buttons(ui, tl!("OK"));
    if remove {
        let _ = app.run("format.phonetic", json!({"ruby": null}));
    }
    let mut applied = false;
    if ok && !remove {
        applied = if *group || !cells {
            if cells {
                *ruby = readings(units, original).join(" ");
            }
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
    fn an_empty_reading_falls_back_to_the_default_one() {
        let original = vec!["yín".to_string(), "xíng".to_string()];
        let units = u(&[("银", " "), ("行", "há ng")]);
        assert_eq!(readings(&units, &original), ["yín", "háng"]);
        let (items, joined) = preview_items(&units, &original, false);
        assert_eq!(items, u(&[("银", "yín"), ("行", "háng")]));
        assert_eq!(joined, None);
        let (_, joined) = preview_items(&units, &original, true);
        assert_eq!(joined.as_deref(), Some("yín háng"));
    }

    #[test]
    fn ruby_alignment_edges() {
        assert_eq!((ruby_x("left", 40.0, 10.0), ruby_x("rightVertical", 40.0, 10.0), ruby_x("distributeSpace", 40.0, 10.0)), (0.0, 30.0, 15.0));
        assert_eq!(ruby_x("right", 10.0, 40.0), 0.0, "a wider reading does not go negative");
    }
}
