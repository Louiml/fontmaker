//! Charsets, joining rules and script metrics for FontMaker.
//!
//! The tables are hand-curated rather than generated from the Unicode database
//! so that the build is fully offline and the data is reviewable. Coverage is
//! intentionally modest: enough letters to set real text, not the whole block.

use fontmaker_core::document::{CharsetEntry, Document, Script, VerticalMetrics};
use fontmaker_core::glyph::{Form, Glyph, JoiningType};

/// A letter of a script: its codepoint and how it joins its neighbours.
///
/// Hebrew letters are all `NonJoining` here on purpose: Hebrew final forms are
/// separate codepoints (׳ ׳ ׳ ׳£ ׳¥), so there are no OpenType `init/medi/fina`
/// forms to generate ג€” one glyph per codepoint is the whole story.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Letter {
    pub code: u32,
    pub joining: JoiningType,
}

const fn nj(code: u32) -> Letter {
    Letter { code, joining: JoiningType::NonJoining }
}

const fn dual(code: u32) -> Letter {
    Letter { code, joining: JoiningType::DualJoining }
}

const fn right(code: u32) -> Letter {
    Letter { code, joining: JoiningType::RightJoining }
}

/// A-Z, a-z.
const LATIN_LETTERS: &[Letter] = &[
    nj(0x41), nj(0x42), nj(0x43), nj(0x44), nj(0x45), nj(0x46), nj(0x47), nj(0x48), nj(0x49),
    nj(0x4A), nj(0x4B), nj(0x4C), nj(0x4D), nj(0x4E), nj(0x4F), nj(0x50), nj(0x51), nj(0x52),
    nj(0x53), nj(0x54), nj(0x55), nj(0x56), nj(0x57), nj(0x58), nj(0x59), nj(0x5A),
    nj(0x61), nj(0x62), nj(0x63), nj(0x64), nj(0x65), nj(0x66), nj(0x67), nj(0x68), nj(0x69),
    nj(0x6A), nj(0x6B), nj(0x6C), nj(0x6D), nj(0x6E), nj(0x6F), nj(0x70), nj(0x71), nj(0x72),
    nj(0x73), nj(0x74), nj(0x75), nj(0x76), nj(0x77), nj(0x78), nj(0x79), nj(0x7A),
];

/// ׀-׀¯, ׀°-ׁ, ׀ and ׁ‘.
const CYRILLIC_LETTERS: &[Letter] = &[
    nj(0x410), nj(0x411), nj(0x412), nj(0x413), nj(0x414), nj(0x415), nj(0x416), nj(0x417),
    nj(0x418), nj(0x419), nj(0x41A), nj(0x41B), nj(0x41C), nj(0x41D), nj(0x41E), nj(0x41F),
    nj(0x420), nj(0x421), nj(0x422), nj(0x423), nj(0x424), nj(0x425), nj(0x426), nj(0x427),
    nj(0x428), nj(0x429), nj(0x42A), nj(0x42B), nj(0x42C), nj(0x42D), nj(0x42E), nj(0x42F),
    nj(0x430), nj(0x431), nj(0x432), nj(0x433), nj(0x434), nj(0x435), nj(0x436), nj(0x437),
    nj(0x438), nj(0x439), nj(0x43A), nj(0x43B), nj(0x43C), nj(0x43D), nj(0x43E), nj(0x43F),
    nj(0x440), nj(0x441), nj(0x442), nj(0x443), nj(0x444), nj(0x445), nj(0x446), nj(0x447),
    nj(0x448), nj(0x449), nj(0x44A), nj(0x44B), nj(0x44C), nj(0x44D), nj(0x44E), nj(0x44F),
    nj(0x401), nj(0x451),
];

/// ־‘-־©, ־±-ֿ‰ with tonos variants and final sigma.
const GREEK_LETTERS: &[Letter] = &[
    nj(0x391), nj(0x392), nj(0x393), nj(0x394), nj(0x395), nj(0x396), nj(0x397), nj(0x398),
    nj(0x399), nj(0x39A), nj(0x39B), nj(0x39C), nj(0x39D), nj(0x39E), nj(0x39F), nj(0x3A0),
    nj(0x3A1), nj(0x3A3), nj(0x3A4), nj(0x3A5), nj(0x3A6), nj(0x3A7), nj(0x3A8), nj(0x3A9),
    nj(0x3B1), nj(0x3B2), nj(0x3B3), nj(0x3B4), nj(0x3B5), nj(0x3B6), nj(0x3B7), nj(0x3B8),
    nj(0x3B9), nj(0x3BA), nj(0x3BB), nj(0x3BC), nj(0x3BD), nj(0x3BE), nj(0x3BF), nj(0x3C0),
    nj(0x3C1), nj(0x3C2), nj(0x3C3), nj(0x3C4), nj(0x3C5), nj(0x3C6), nj(0x3C7), nj(0x3C8),
    nj(0x3C9),
];

/// The Arabic block 0621-064A with joining classes from Unicode's
/// ArabicShaping.txt, plus the tatweel elongation character.
const ARABIC_LETTERS: &[Letter] = &[
    nj(0x0621),                    // ״¡ hamza
    right(0x0622), right(0x0623),  // ״¢ ״£
    dual(0x0624),                  // ״₪
    right(0x0625),                 // ״¥
    dual(0x0626),                  // ״¦
    right(0x0627),                 // ״§
    dual(0x0628),                  // ״¨
    right(0x0629),                 // ״©
    dual(0x062A), dual(0x062B), dual(0x062C), dual(0x062D), dual(0x062E),
    right(0x062F), right(0x0630), right(0x0631), right(0x0632), // ״¯ ״° ״± ״²
    dual(0x0633), dual(0x0634), dual(0x0635), dual(0x0636), dual(0x0637),
    dual(0x0638), dual(0x0639), dual(0x063A),
    dual(0x0640),                  // € tatweel
    dual(0x0641), dual(0x0642), dual(0x0643), dual(0x0644), dual(0x0645),
    dual(0x0646), dual(0x0647),
    right(0x0648), right(0x0649),  // ˆ ‰
    dual(0x064A),                  // 
];

/// Hebrew letters, including the five final forms.
const HEBREW_LETTERS: &[Letter] = &[
    nj(0x05D0), nj(0x05D1), nj(0x05D2), nj(0x05D3), nj(0x05D4), nj(0x05D5), nj(0x05D6),
    nj(0x05D7), nj(0x05D8), nj(0x05D9), nj(0x05DA), nj(0x05DB), nj(0x05DC), nj(0x05DD),
    nj(0x05DE), nj(0x05DF), nj(0x05E0), nj(0x05E1), nj(0x05E2), nj(0x05E3), nj(0x05E4),
    nj(0x05E5), nj(0x05E6), nj(0x05E7), nj(0x05E8), nj(0x05E9), nj(0x05EA),
];

/// Digits and punctuation shared by every script. Numbers and basic
/// punctuation make a font usable even while the letters are still being drawn.
const SHARED: &[Letter] = &[
    nj(0x30), nj(0x31), nj(0x32), nj(0x33), nj(0x34),
    nj(0x35), nj(0x36), nj(0x37), nj(0x38), nj(0x39),
    nj(0x2E), nj(0x2C), nj(0x3A), nj(0x3B), nj(0x21), nj(0x3F),
    nj(0x2D), nj(0x28), nj(0x29), nj(0x22), nj(0x27),
];

/// The letters of each script, not counting the shared digits and punctuation.
pub fn letters(script: Script) -> &'static [Letter] {
    match script {
        Script::Latin => LATIN_LETTERS,
        Script::Cyrillic => CYRILLIC_LETTERS,
        Script::Greek => GREEK_LETTERS,
        Script::Arabic => ARABIC_LETTERS,
        Script::Hebrew => HEBREW_LETTERS,
    }
}

/// The PostScript-ish glyph name for a codepoint.
///
/// ASCII letters keep their own character as the name, digits and punctuation
/// use the Adobe Glyph List names, and everything else becomes `uniXXXX`.
pub fn glyph_name(code: u32) -> String {
    match code {
        0x20 => "space".to_string(),
        0x2D => "hyphen".to_string(),
        0x2E => "period".to_string(),
        0x2C => "comma".to_string(),
        0x3A => "colon".to_string(),
        0x3B => "semicolon".to_string(),
        0x21 => "exclam".to_string(),
        0x3F => "question".to_string(),
        0x28 => "parenleft".to_string(),
        0x29 => "parenright".to_string(),
        0x22 => "quotedbl".to_string(),
        0x27 => "quotesingle".to_string(),
        0x30..=0x39 => ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"]
                [(code - 0x30) as usize].to_string(),
        c @ (0x41..=0x5A | 0x61..=0x7A) => char::from_u32(c).unwrap().to_string(),
        _ => format!("uni{code:04X}"),
    }
}

/// The full charset for a script: letters plus the shared digits/punctuation.
pub fn charset(script: Script) -> Vec<CharsetEntry> {
    letters(script)
        .iter()
        .chain(SHARED)
        .map(|l| CharsetEntry {
            codepoint: l.code,
            glyph: glyph_name(l.code),
            joining: l.joining,
            transparent: false,
        })
        .collect()
}

/// Default vertical metrics per script. Arabic needs a taller em box because
/// ligature stacks and marks climb higher than Latin capitals.
pub fn metrics(script: Script) -> VerticalMetrics {
    match script {
        Script::Arabic => VerticalMetrics {
            ascender: 900.0,
            descender: -450.0,
            cap_height: 700.0,
            x_height: 550.0,
            ..VerticalMetrics::default()
        },
        _ => VerticalMetrics::default(),
    }
}

/// A short line of real text to preview with, per script.
pub fn preview_text(script: Script) -> &'static str {
    match script {
        Script::Latin => "Handgloves 123",
        Script::Cyrillic => "׀ ׁƒ׀÷׀°׀²׀¸ׁ†ׁ‹ 123",
        Script::Greek => "־§־µ־¹־¼ֿ־½־µֿ‚ 123",
        Script::Arabic => "״³„״§… ״¹„ƒ…",
        Script::Hebrew => "׳©׳׳•׳ 123",
    }
}

/// The Unicode value of a glyph name of the form `uniXXXX`, if it is one.
pub fn codepoint_of_uni_name(name: &str) -> Option<u32> {
    name.strip_prefix("uni")
        .filter(|rest| rest.len() == 4)
        .and_then(|rest| u32::from_str_radix(rest, 16).ok())
}

/// Fill a document with the glyphs a script needs.
///
/// Existing glyphs are kept. For Arabic, the positional forms of every joining
/// letter are created as separate (initially empty) glyphs so they appear in
/// the glyph grid and can be drawn one by one.
pub fn apply_script(doc: &mut Document, script: Script) {
    doc.script = script;
    doc.metrics = metrics(script);
    doc.charset = charset(script);

    // Clone the names first: the loop below mutates the document.
    let names: Vec<(String, Option<char>)> = doc
        .charset
        .iter()
        .map(|e| {
            (
                e.glyph.clone(),
                char::from_u32(e.codepoint),
            )
        })
        .collect();
    for (name, unicode) in names {
        create_base_and_forms(doc, name, unicode);
    }

    // Keep .notdef and space at the head of the order.
    doc.glyph_order.retain(|name| name != ".notdef");
    let mut order = vec![".notdef".to_string()];
    order.append(&mut doc.glyph_order);
    doc.glyph_order = order;
}

/// Create a glyph and, for a joining letter, its positional forms.
fn create_base_and_forms(doc: &mut Document, name: String, unicode: Option<char>) {
    let joining = doc
        .charset
        .iter()
        .find(|e| e.glyph == name)
        .map(|e| e.joining)
        .unwrap_or(JoiningType::NonJoining);

    if !doc.glyphs.contains_key(&name) {
        let mut glyph = Glyph::new(name.clone());
        glyph.unicode = unicode.map(|c| vec![c]).unwrap_or_default();
        doc.add_glyph(glyph);
    }

    if !doc.script.uses_forms() {
        return;
    }
    let forms: &[Form] = match joining {
        JoiningType::DualJoining => &Form::ALL[1..],
        JoiningType::RightJoining => &[Form::Fina],
        _ => &[],
    };
    for form in forms {
        let form_name = format!("{name}{}", form.suffix());
        if !doc.glyphs.contains_key(&form_name) {
            doc.add_glyph(Glyph::new(form_name));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_agl_for_ascii() {
        assert_eq!(glyph_name(0x41), "A");
        assert_eq!(glyph_name(0x7A), "z");
        assert_eq!(glyph_name(0x30), "zero");
        assert_eq!(glyph_name(0x2E), "period");
        assert_eq!(glyph_name(0x20), "space");
        assert_eq!(glyph_name(0x0627), "uni0627");
    }

    #[test]
    fn charset_includes_shared_punctuation() {
        let cs = charset(Script::Latin);
        assert!(cs.iter().any(|e| e.codepoint == 0x30), "digits");
        assert!(cs.iter().any(|e| e.codepoint == 0x2E), "period");
        assert!(cs.iter().any(|e| e.codepoint == 0x41), "letters");
    }

    #[test]
    fn arabic_joining_classes() {
        let cs = charset(Script::Arabic);
        let joining = |code: u32| cs.iter().find(|e| e.codepoint == code).unwrap().joining;
        assert_eq!(joining(0x0628), JoiningType::DualJoining, "״¨ joins both sides");
        assert_eq!(joining(0x0627), JoiningType::RightJoining, "״§ joins right only");
        assert_eq!(joining(0x0621), JoiningType::NonJoining, "״¡ never joins");
        assert_eq!(joining(0x0640), JoiningType::DualJoining, "tatweel joins both");
    }

    #[test]
    fn apply_script_creates_arabic_forms() {
        let mut doc = Document::default();
        apply_script(&mut doc, Script::Arabic);
        // ״¨ is dual-joining, so it gets initial, medial and final forms.
        assert!(doc.glyph("uni0628").is_some());
        assert!(doc.glyph("uni0628.init").is_some());
        assert!(doc.glyph("uni0628.medi").is_some());
        assert!(doc.glyph("uni0628.fina").is_some());
        // ״§ is right-joining: only a final form.
        assert!(doc.glyph("uni0627").is_some());
        assert!(doc.glyph("uni0627.fina").is_some());
        assert!(doc.glyph("uni0627.init").is_none());
        // Hamza never joins: no forms at all.
        assert!(doc.glyph("uni0621.init").is_none());
        // .notdef stays first.
        assert_eq!(doc.glyph_order[0], ".notdef");
    }

    #[test]
    fn hebrew_has_no_forms() {
        let mut doc = Document::default();
        apply_script(&mut doc, Script::Hebrew);
        assert!(doc.glyph("uni05DB").is_some(), "regular kaf");
        assert!(doc.glyph("uni05DA").is_some(), "final kaf is its own codepoint");
        assert!(doc.glyph("uni05DB.init").is_none());
    }

    #[test]
    fn reapplying_keeps_drawn_glyphs() {
        let mut doc = Document::default();
        apply_script(&mut doc, Script::Latin);
        doc.glyph_mut("A").unwrap().contours.push(
            fontmaker_core::stroke::rectangle(0.0, 0.0, 100.0, 100.0),
        );
        apply_script(&mut doc, Script::Latin);
        assert!(doc.glyph("A").unwrap().is_drawn(), "glyph must survive a re-apply");
        assert_eq!(doc.completion().1, doc.charset.len());
    }

    #[test]
    fn unicode_name_roundtrip() {
        assert_eq!(codepoint_of_uni_name("uni0627"), Some(0x0627));
        assert_eq!(codepoint_of_uni_name("A"), None);
        assert_eq!(codepoint_of_uni_name("uniXYZ"), None);
    }
}