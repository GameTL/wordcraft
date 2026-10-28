//! Home › Font › Phonetic Guide: ruby text (furigana, pinyin…) over base text, kept as one
//! [`InlineObject::Ruby`] in the paragraph.

use pinyin::ToPinyin;
use serde_json::{Value, json};
use wordcraft_doc::para::{OBJ, RubyAlign, ruby_offset, ruby_raise};
use wordcraft_doc::{InlineObject, Pos};

use crate::{CmdError, CmdResult, CommandSpec, Selection, Session, p};

/// Longest base or ruby text, characters.
const MAX_CHARS: usize = 255;
/// Most per-character guides one Mono Ruby run makes.
const MAX_GUIDES: usize = 20_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("format.phonetic", "Phonetic Guide", "Home › Font", phonetic).params(
            r#"{"ruby"?: string | null, "mono"?: bool, "align"?: "center|distributeLetter|distributeSpace|left|right|rightVertical", "size"?: number, "offset"?: number} — mono: one guide per Chinese character, readings from `ruby` (space-separated, one per character) or else pinyin"#,
        ),
    ]
}

/// The pinyin reading of a Han character, tone marks included (`中` → `zhōng`).
// ponytail: per-character readings with no phrase context (行 in 银行 gets xíng, not háng; 了 is
// always le). The user corrects them in the dialog; upgrade path is a phrase dictionary (the
// crate's heteronym feature plus a word table).
fn pinyin_of(c: char) -> Option<&'static str> {
    c.to_pinyin().map(|p| p.with_tone())
}

fn kana(c: char) -> bool {
    matches!(c, '\u{3040}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}' | '\u{FF66}'..='\u{FF9F}')
}

/// Letters pinyin is written in (ASCII and the Latin extensions with the tone marks).
fn pinyin_letter(c: char) -> bool {
    matches!(c, 'a'..='z' | 'A'..='Z' | '\u{C0}'..='\u{24F}' | '\u{1E00}'..='\u{1EFF}')
}

/// Is the text at `at` Japanese (kana in it, or a Japanese language there or in the interface)?
/// Its kanji then take Japanese readings, which pinyin isn't.
fn japanese(s: &Session, text: &str, at: &Pos) -> bool {
    let ja = |l: Option<&str>| l.and_then(|l| l.get(..2)).is_some_and(|l| l.eq_ignore_ascii_case("ja"));
    if text.chars().any(kana) || ja(s.editing_lang.as_deref()) {
        return true;
    }
    let Some(para) = s.doc.para_at(at) else { return false };
    let c = para.props_of_char(at.off);
    ja(c.lang.as_deref()) || ja(c.lang_ea.as_deref()) || ja(s.doc.styles.resolve_char(para.props.style.as_deref(), c).lang.as_deref())
}

/// The pinyin the dialog suggests for `base` at `at`: one reading per Chinese character, space
/// separated; empty for Japanese text.
fn suggest(s: &Session, base: &str, at: &Pos) -> String {
    if japanese(s, base, at) {
        return String::new();
    }
    base.chars().filter_map(pinyin_of).collect::<Vec<_>>().join(" ")
}

/// Ruby text as typed: no control characters or object marks, at most [`MAX_CHARS`].
fn clean(t: &str) -> String {
    t.chars().filter(|c| !c.is_control() && *c != OBJ).take(MAX_CHARS).collect()
}

/// Sets the selected text's ruby (one group over the whole selection, or the phonetic guide the
/// selection is or the caret touches); `null` or `""` removes it. Without `ruby`, opens the
/// dialog and returns the selection's phonetic guide (`base`, `ruby`, `align`, `size`, `offset`).
/// `mono` gives each Chinese character of the selection its own guide instead (Word's Mono Ruby).
fn phonetic(s: &mut Session, v: &Value) -> CmdResult {
    let align = match p::str(v, "align") {
        Some(a) => Some(RubyAlign::from_ooxml(a).ok_or_else(|| CmdError::Params(format!("unknown alignment `{a}`")))?),
        None => None,
    };
    let size = p::f32(v, "size").map(|x| x.clamp(1.0, 1584.0));
    let offset = p::f32(v, "offset").map(|x| x.clamp(-1584.0, 1584.0));
    if p::bool(v, "mono") == Some(true) {
        let readings = match v.get("ruby") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::String(t)) => t.split_whitespace().map(clean).collect(),
            _ => return Err(CmdError::Params("`ruby` must be a string or null".into())),
        };
        return mono(s, readings, align, size, offset);
    }
    let Some(rv) = v.get("ruby") else {
        s.push_ui_request(json!({"open": "phonetic"}));
        return Ok(state(s));
    };
    let text = match rv {
        Value::Null => String::new(),
        Value::String(t) => clean(t),
        _ => return Err(CmdError::Params("`ruby` must be a string or null".into())),
    };
    let text = text.trim().to_string();
    if let Some(at) = ruby_at(s) {
        let end = Pos { off: at.off + OBJ.len_utf8(), ..at.clone() };
        let bsz = base_size(s, &at);
        let para = s.doc.para_mut(at.story, &at.path)?;
        let props = para.props_of_char(at.off).clone();
        match para.object_at_mut(at.off) {
            Some(InlineObject::Ruby { base, .. }) if text.is_empty() => {
                // Remove: the base text stays, in the ruby's formatting.
                let base = base.clone();
                s.doc.delete_range(&at, &end)?;
                let stop = s.doc.insert_text(&at, &base, &props)?;
                s.sel = Selection { anchor: at, focus: stop };
                return Ok(state(s));
            }
            Some(InlineObject::Ruby { ruby, align: al, size: sz, raise, base_size, .. }) => {
                let bsz = if *base_size > 0.0 { *base_size } else { bsz };
                let keep = ruby_offset(*raise, bsz, *sz);
                *ruby = text;
                *al = align.unwrap_or(*al);
                *sz = size.unwrap_or(*sz);
                *raise = ruby_raise(offset.unwrap_or(keep), bsz, *sz).clamp(0.0, 1584.0);
                *base_size = bsz;
            }
            _ => return Err(CmdError::Failed("no phonetic guide at the selection".into())),
        }
        para.touch();
        s.sel = Selection { anchor: at, focus: end };
        return Ok(state(s));
    }
    if text.is_empty() {
        return Err(CmdError::Failed("no phonetic guide at the selection".into()));
    }
    let (a, b) = s.sel.ordered();
    if a == b || a.story != b.story || a.path != b.path {
        return Err(CmdError::Failed("select the text, within one paragraph, to set a phonetic guide over".into()));
    }
    let para = s.doc.para_at(&a).ok_or_else(|| CmdError::Failed("no paragraph at the selection".into()))?;
    let base = para.text.get(a.off..b.off).unwrap_or("").to_string();
    if base.is_empty() || base.chars().any(|c| c == OBJ || c.is_control()) {
        return Err(CmdError::Failed("a phonetic guide goes over plain text (no objects, tabs or breaks)".into()));
    }
    if base.chars().count() > MAX_CHARS {
        return Err(CmdError::Failed(format!("a phonetic guide covers at most {MAX_CHARS} characters")));
    }
    // One set of formatting draws the whole guide: a symbol font only when every base character
    // has it, else the symbol's font would turn the other letters and the guide into symbols.
    let first = para.props_of_char(a.off).clone();
    let all_symbols = base.char_indices().all(|(i, _)| para.props_of_char(a.off + i).sym_font == first.sym_font);
    let props = if all_symbols { first } else { first.without_symbol() };
    let bsz = base_size(s, &a);
    let size = size.unwrap_or_else(|| default_size(bsz));
    let raise = ruby_raise(offset.unwrap_or(0.0), bsz, size).clamp(0.0, 1584.0);
    // ponytail: Chinese base with Latin-letter ruby is taken as pinyin (`zh-CN`); anything else
    // keeps the empty language, which DOCX writes as Japanese. Upgrade: the run's `lang_ea`.
    let lang = if base.chars().any(|c| pinyin_of(c).is_some()) && text.chars().any(pinyin_letter) { "zh-CN" } else { "" };
    let obj = InlineObject::Ruby {
        base,
        ruby: text,
        align: align.unwrap_or_default(),
        size,
        raise,
        base_size: bsz,
        lang: lang.into(),
        props: Default::default(),
    };
    s.doc.delete_range(&a, &b)?;
    let end = s.doc.insert_object(&a, obj, &props)?;
    s.sel = Selection { anchor: a, focus: end };
    Ok(state(s))
}

/// Mono Ruby: a guide over each Chinese character of the selection (which may span paragraphs
/// of one story). `readings` go to those characters in order; empty, each gets its pinyin.
/// Existing guides, pictures, tabs and other text stay, so a second run only fills the gaps.
fn mono(s: &mut Session, readings: Vec<String>, align: Option<RubyAlign>, size: Option<f32>, offset: Option<f32>) -> CmdResult {
    let (a, b) = s.sel.ordered();
    if a == b {
        return Err(CmdError::Failed("select the Chinese text to add pinyin to".into()));
    }
    if a.story != b.story {
        return Err(CmdError::Failed("the selection spans stories".into()));
    }
    // Every (paragraph, offset, character) to put a guide over, found before the first edit.
    let mut targets: Vec<(wordcraft_doc::Path, usize, char)> = Vec::new();
    for path in s.doc.para_paths(a.story).into_iter().filter(|p| *p >= a.path && *p <= b.path) {
        let Some(para) = s.doc.para(a.story, &path) else { continue };
        let from = if path == a.path { a.off } else { 0 };
        let to = if path == b.path { b.off } else { para.len() };
        let Some(text) = para.text.get(from..to) else { continue };
        for (i, c) in text.char_indices().filter(|(_, c)| pinyin_of(*c).is_some()) {
            if targets.len() >= MAX_GUIDES {
                return Err(CmdError::Failed(format!("Mono Ruby covers at most {MAX_GUIDES} characters at a time")));
            }
            targets.push((path.clone(), from + i, c));
        }
    }
    if targets.is_empty() {
        return Err(CmdError::Failed("no Chinese characters with a pinyin reading in the selection".into()));
    }
    if !readings.is_empty() && readings.len() != targets.len() {
        return Err(CmdError::Params(format!("{} readings for {} Chinese characters", readings.len(), targets.len())));
    }
    let mut focus = b.clone();
    // Backwards, so the offsets still to come stay valid.
    for (k, (path, off, c)) in targets.into_iter().enumerate().rev() {
        let ruby = match readings.get(k) {
            Some(r) => r.clone(),
            None => pinyin_of(c).unwrap_or_default().to_string(),
        };
        let at = Pos { story: a.story, path, off };
        let bsz = base_size(s, &at);
        let props = s.doc.para_at(&at).map(|p| p.props_of_char(off).clone()).unwrap_or_default();
        let props = if props.sym_font.is_some() { props.without_symbol() } else { props };
        let size = size.unwrap_or_else(|| default_size(bsz));
        let obj = InlineObject::Ruby {
            base: c.to_string(),
            ruby,
            align: align.unwrap_or_default(),
            size,
            raise: ruby_raise(offset.unwrap_or(0.0), bsz, size).clamp(0.0, 1584.0),
            base_size: bsz,
            lang: "zh-CN".into(),
            props: Default::default(),
        };
        let end = Pos { off: off + c.len_utf8(), ..at.clone() };
        s.doc.delete_range(&at, &end)?;
        s.doc.insert_object(&at, obj, &props)?;
        if at.path == b.path {
            focus.off = (focus.off + OBJ.len_utf8()).saturating_sub(c.len_utf8());
        }
    }
    s.sel = Selection { anchor: a, focus };
    Ok(state(s))
}

/// The phonetic guide the selection is (exactly one ruby object) or the caret is next to.
fn ruby_at(s: &Session) -> Option<Pos> {
    let (a, b) = s.sel.ordered();
    let para = s.doc.para_at(&a)?;
    let ruby = |off: usize| matches!(para.object_at(off), Some(InlineObject::Ruby { .. })).then(|| Pos { off, ..a.clone() });
    let w = OBJ.len_utf8();
    if a == b {
        return ruby(a.off).or_else(|| a.off.checked_sub(w).and_then(ruby));
    }
    (a.story == b.story && a.path == b.path && b.off == a.off.saturating_add(w)).then(|| ruby(a.off)).flatten()
}

/// The resolved font size of the character at `at`, points.
fn base_size(s: &Session, at: &Pos) -> f32 {
    let Some(para) = s.doc.para_at(at) else { return 10.5 };
    let size = s.doc.styles.resolve_char(para.props.style.as_deref(), para.props_of_char(at.off)).size;
    if size.is_finite() { size.clamp(1.0, 1584.0) } else { 10.5 }
}

/// Ruby half the base size, rounded down to a half point (10.5 pt text gets 5 pt ruby).
fn default_size(base: f32) -> f32 {
    (base.floor() / 2.0).max(1.0)
}

/// What the Phonetic Guide shows for the selection: its ruby, or the selected text with defaults
/// (and its pinyin when it's Chinese).
pub fn state(s: &Session) -> Value {
    if let Some(at) = ruby_at(s)
        && let Some(InlineObject::Ruby { base, ruby, align, size, raise, base_size: bs, .. }) = s.doc.para_at(&at).and_then(|p| p.object_at(at.off))
    {
        let bs = if *bs > 0.0 { *bs } else { base_size(s, &at) };
        // `+ 0.0` turns a rounded -0 into 0.
        let offset = (ruby_offset(*raise, bs, *size) * 10.0).round() / 10.0 + 0.0;
        return json!({"base": base, "ruby": ruby, "align": align.ooxml(), "size": size, "offset": offset, "existing": true});
    }
    let (a, b) = s.sel.ordered();
    let base = if a.story == b.story && a.path == b.path { s.selected_text() } else { String::new() };
    let base: String = base.chars().filter(|c| !c.is_control() && *c != OBJ).take(MAX_CHARS).collect();
    let ruby = suggest(s, &base, &a);
    json!({"base": base, "ruby": ruby, "align": "center", "size": default_size(base_size(s, &a)), "offset": 0.0, "existing": false})
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wordcraft_doc::{Document, InlineObject, Paragraph, StoryRef};

    use crate::Session;

    fn session(text: &str) -> Session {
        let mut d = Document::new();
        d.body = vec![wordcraft_doc::para_block(Paragraph::with_text(text, Default::default()))];
        Session::new(d)
    }

    fn para(s: &Session) -> Paragraph {
        s.doc.para(StoryRef::Body, &wordcraft_doc::Path::top(0)).cloned().unwrap()
    }

    /// #2036: a guide over text that starts with a symbol doesn't draw the rest (and the guide)
    /// in the symbol font; a guide over symbols only keeps their font.
    #[test]
    fn a_phonetic_guide_takes_a_symbol_font_only_over_symbols() {
        for (text, kept) in [("\u{F0FC}ab", None), ("\u{F0FC}\u{F0FB}", Some("Wingdings".to_string()))] {
            let mut s = session(text);
            let end = wordcraft_doc::Pos::body(0, text.len());
            s.doc.format_range(&wordcraft_doc::Pos::body(0, 0), &end, &|c| c.sym_font = Some("Wingdings".into())).unwrap();
            if kept.is_none() {
                s.doc.format_range(&wordcraft_doc::Pos::body(0, 3), &end, &|c| c.sym_font = None).unwrap();
            }
            s.run("select.text", &json!({"text": text})).unwrap();
            s.run("format.phonetic", &json!({"ruby": "r"})).unwrap();
            assert_eq!(para(&s).props_of_char(0).sym_font, kept, "{text:?}");
        }
    }

    #[test]
    fn phonetic_guide_applies_edits_removes_and_undoes() {
        let mut s = session("I read 漢字 daily.");
        s.run("select.text", &json!({"text": "漢字"})).unwrap();
        let r = s.run("format.phonetic", &json!({"ruby": "かんじ", "align": "distributeSpace"})).unwrap();
        assert_eq!((r["base"].as_str(), r["ruby"].as_str(), r["existing"].as_bool()), (Some("漢字"), Some("かんじ"), Some(true)));
        let p = para(&s);
        assert_eq!(p.plain_text(), "I read 漢字 daily.");
        let Some(InlineObject::Ruby { ruby, size, align, .. }) = p.objects.first() else { panic!("{:?}", p.objects) };
        assert_eq!((ruby.as_str(), *size, align.ooxml()), ("かんじ", 6.0, "distributeSpace"));
        // The selection is the ruby: editing it keeps the base, changes the reading and offset.
        let r = s.run("format.phonetic", &json!({"ruby": "カンジ", "offset": 2})).unwrap();
        assert_eq!((r["ruby"].as_str(), r["offset"].as_f64()), (Some("カンジ"), Some(2.0)));
        assert_eq!(para(&s).objects.len(), 1);
        // Remove: the base text is plain text again, selected.
        s.run("format.phonetic", &json!({"ruby": null})).unwrap();
        assert!(para(&s).objects.is_empty());
        assert_eq!(s.selected_text(), "漢字");
        // Undo brings the edited ruby back, then the first one, then the plain text.
        s.run("edit.undo", &json!({})).unwrap();
        assert!(matches!(para(&s).objects.first(), Some(InlineObject::Ruby { ruby, .. }) if ruby == "カンジ"));
        s.run("edit.undo", &json!({})).unwrap();
        s.run("edit.undo", &json!({})).unwrap();
        assert!(para(&s).objects.is_empty());
        assert_eq!(para(&s).plain_text(), "I read 漢字 daily.");
        // Without `ruby` it reports the selection (and asks the UI for the dialog).
        s.run("select.text", &json!({"text": "read"})).unwrap();
        let r = s.run("format.phonetic", &json!({})).unwrap();
        assert_eq!((r["base"].as_str(), r["existing"].as_bool()), (Some("read"), Some(false)));
        assert!(s.run("format.phonetic", &json!({"ruby": "x", "align": "sideways"})).is_err());
    }

    fn rubies(p: &Paragraph) -> Vec<(String, String, String)> {
        p.objects
            .iter()
            .filter_map(|o| match o {
                InlineObject::Ruby { base, ruby, lang, .. } => Some((base.clone(), ruby.clone(), lang.clone())),
                _ => None,
            })
            .collect()
    }

    /// #3384: Mono Ruby puts each Chinese character's pinyin over it, leaves Latin text alone,
    /// and undoes in one step.
    #[test]
    fn mono_ruby_adds_pinyin_per_character() {
        let mut s = session("我爱中国a");
        s.run("select.all", &json!({})).unwrap();
        s.run("format.phonetic", &json!({"mono": true})).unwrap();
        let p = para(&s);
        let got: Vec<_> = rubies(&p).into_iter().map(|(b, r, l)| format!("{b}:{r}:{l}")).collect();
        assert_eq!(got, ["我:wǒ:zh-CN", "爱:ài:zh-CN", "中:zhōng:zh-CN", "国:guó:zh-CN"]);
        assert_eq!(p.plain_text(), "我爱中国a");
        assert!(p.text.ends_with('a'), "the Latin letter stays text");
        assert_eq!(s.selected_text(), "我爱中国a", "the selection still covers the text");
        s.run("edit.undo", &json!({})).unwrap();
        assert!(para(&s).objects.is_empty());
        assert_eq!(para(&s).text, "我爱中国a");
    }

    /// #3384: the teacher's corrections go to the characters in order; a count mismatch is refused.
    #[test]
    fn mono_ruby_takes_corrected_readings() {
        let mut s = session("银行");
        s.run("select.all", &json!({})).unwrap();
        assert!(s.run("format.phonetic", &json!({"mono": true, "ruby": "yín"})).is_err());
        assert!(para(&s).objects.is_empty());
        s.run("format.phonetic", &json!({"mono": true, "ruby": "yín  háng"})).unwrap();
        let got: Vec<_> = rubies(&para(&s)).into_iter().map(|(_, r, _)| r).collect();
        assert_eq!(got, ["yín", "háng"]);
    }

    /// #3384: the dialog opens with the pinyin filled in, but not over Japanese text.
    #[test]
    fn the_dialog_suggests_pinyin_for_chinese_only() {
        let mut s = session("我爱中国");
        s.run("select.text", &json!({"text": "中国"})).unwrap();
        let r = s.run("format.phonetic", &json!({})).unwrap();
        assert_eq!((r["ruby"].as_str(), r["existing"].as_bool()), (Some("zhōng guó"), Some(false)));
        let mut s = session("漢字かな");
        s.run("select.all", &json!({})).unwrap();
        assert_eq!(s.run("format.phonetic", &json!({})).unwrap()["ruby"], "", "kana: Japanese");
        let mut s = session("漢字");
        s.doc.styles.default_chr.lang = Some("ja-JP".into());
        s.run("select.all", &json!({})).unwrap();
        assert_eq!(s.run("format.phonetic", &json!({})).unwrap()["ruby"], "", "a Japanese document");
    }

    /// #3384: Mono Ruby across paragraphs, and again over text that already has a guide.
    #[test]
    fn mono_ruby_spans_paragraphs_and_fills_gaps() {
        let mut d = Document::new();
        d.body = ["中国", "你好"].iter().map(|t| wordcraft_doc::para_block(Paragraph::with_text(t, Default::default()))).collect();
        let mut s = Session::new(d);
        s.run("select.all", &json!({})).unwrap();
        s.run("format.phonetic", &json!({"mono": true})).unwrap();
        let second = s.doc.para(StoryRef::Body, &wordcraft_doc::Path::top(1)).cloned().unwrap();
        assert_eq!(rubies(&para(&s)).len(), 2);
        assert_eq!(rubies(&second).into_iter().map(|(_, r, _)| r).collect::<Vec<_>>(), ["nǐ", "hǎo"]);

        let mut s = session("银行");
        s.run("select.text", &json!({"text": "行"})).unwrap();
        s.run("format.phonetic", &json!({"mono": true, "ruby": "háng"})).unwrap();
        s.run("select.all", &json!({})).unwrap();
        s.run("format.phonetic", &json!({"mono": true})).unwrap();
        let got: Vec<_> = rubies(&para(&s)).into_iter().map(|(b, r, _)| format!("{b}:{r}")).collect();
        assert_eq!(got, ["银:yín", "行:háng"], "the corrected guide stays");
        assert!(s.run("format.phonetic", &json!({"mono": true})).is_err(), "nothing left to add");
    }

    /// #3384: a pinyin group guide is Chinese (`zh-CN`), so DOCX doesn't call it Japanese.
    #[test]
    fn a_pinyin_group_guide_is_chinese_and_survives_docx() {
        let mut s = session("中国");
        s.run("select.all", &json!({})).unwrap();
        s.run("format.phonetic", &json!({"ruby": "zhōngguó"})).unwrap();
        assert_eq!(rubies(&para(&s)), [("中国".to_string(), "zhōngguó".to_string(), "zh-CN".to_string())]);
        let doc = wordcraft_docx::read(&wordcraft_docx::write(&s.doc).unwrap()).unwrap();
        let p = doc.para(StoryRef::Body, &wordcraft_doc::Path::top(0)).unwrap();
        assert_eq!(rubies(p), [("中国".to_string(), "zhōngguó".to_string(), "zh-CN".to_string())]);
        let mut s = session("漢字");
        s.run("select.all", &json!({})).unwrap();
        s.run("format.phonetic", &json!({"ruby": "かんじ"})).unwrap();
        assert_eq!(rubies(&para(&s))[0].2, "", "kana ruby stays Japanese");
    }
}
