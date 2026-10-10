//! OpenType layout writers: GSUB for Arabic positional forms, GPOS for kerning.
//!
//! Both tables share the same skeleton — a ScriptList naming the document's
//! script, a FeatureList naming the features, and a LookupList doing the work.
//! The substitution mappings use `SingleSubstFormat2` (explicit pairs) because
//! our glyph order gives no guarantee that base and form glyphs are adjacent.
//! Kerning uses `PairPosFormat1` with one `PairSet` per left glyph.

use fontmaker_core::document::Document;
use font_types::{GlyphId16, Tag};
use write_fonts::tables::gsub::{
    Gsub, SingleSubst, SingleSubstFormat2, SubstitutionLookup, SubstitutionLookupList,
};
use write_fonts::tables::gpos::{
    Gpos, PairPos, PairPosFormat1, PairSet, PairValueRecord, PositionLookup, PositionLookupList,
    ValueRecord,
};
use write_fonts::tables::layout::{
    CoverageTable, Feature, FeatureList, FeatureRecord, LangSys, Lookup, LookupFlag, Script,
    ScriptList, ScriptRecord,
};

/// Maps a glyph name to its glyph id in export order.
pub type GidOf<'a> = &'a dyn Fn(&str) -> Option<u16>;

/// One feature: its OpenType tag and the (base, form) glyph name pairs.
struct SubstFeature {
    tag: &'static [u8; 4],
    pairs: Vec<(String, String)>,
}

/// Build the GSUB table for Arabic positional forms.
///
/// The isolated form is the glyph the cmap points at, so only `init`, `medi`
/// and `fina` need substitutions. A pair is kept when the *form* glyph has
/// outlines: joining onto a drawn form is worthwhile even if the user has not
/// drawn the isolated variant yet.
pub fn build_gsub(doc: &Document, drawn: &dyn Fn(&str) -> bool, gid_of: GidOf<'_>) -> Option<Gsub> {
    if !doc.script.uses_forms() {
        return None;
    }

    let mut features: Vec<SubstFeature> = Vec::new();
    for (form, tag) in [
        (fontmaker_core::Form::Init, b"init"),
        (fontmaker_core::Form::Medi, b"medi"),
        (fontmaker_core::Form::Fina, b"fina"),
    ] {
        let mut pairs = Vec::new();
        for entry in &doc.charset {
            let letter_has_form = match form {
                fontmaker_core::Form::Init | fontmaker_core::Form::Medi => {
                    matches!(entry.joining, fontmaker_core::JoiningType::DualJoining)
                }
                fontmaker_core::Form::Fina => matches!(
                    entry.joining,
                    fontmaker_core::JoiningType::DualJoining | fontmaker_core::JoiningType::RightJoining
                ),
                fontmaker_core::Form::Isol => false,
            };
            if !letter_has_form {
                continue;
            }
            pairs.push((
                entry.glyph.clone(),
                format!("{}{}", entry.glyph, form.suffix()),
            ));
        }
        features.push(SubstFeature { tag, pairs });
    }
    features.retain(|f| f.pairs.iter().any(|(_, form)| drawn(form)));
    if features.is_empty() {
        return None;
    }

    let mut lookups: Vec<SubstitutionLookup> = Vec::new();
    let mut feature_records: Vec<FeatureRecord> = Vec::new();
    for feature in &features {
        // Sorted coverage, substitutes in coverage order.
        let mut mapped: Vec<(u16, u16)> = feature
            .pairs
            .iter()
            .filter(|(_, form)| drawn(form))
            .filter_map(|(base, form)| Some((gid_of(base)?, gid_of(form)?)))
            .collect();
        mapped.sort();
        mapped.dedup();
        if mapped.is_empty() {
            continue;
        }
        let coverage: CoverageTable = mapped.iter().map(|(g, _)| GlyphId16::new(*g)).collect();
        let substitutes = mapped.iter().map(|(_, g)| GlyphId16::new(*g)).collect();
        let subtable = SingleSubst::Format2(SingleSubstFormat2::new(coverage, substitutes));
        let lookup_index = lookups.len() as u16;
        lookups.push(SubstitutionLookup::Single(Lookup::new(
            LookupFlag::default(),
            vec![subtable],
        )));
        feature_records.push(FeatureRecord::new(
            Tag::new(feature.tag),
            Feature::new(None, vec![lookup_index]),
        ));
    }
    if lookups.is_empty() {
        return None;
    }

    Some(Gsub::new(
        script_list_for(doc, feature_records.len()),
        FeatureList::new(feature_records),
        SubstitutionLookupList::new(lookups),
    ))
}

/// Build the GPOS table for kerning pairs.
pub fn build_gpos_kern(doc: &Document, gid_of: GidOf<'_>) -> Option<Gpos> {
    if doc.kerning.is_empty() {
        return None;
    }

    // Group pairs by left glyph id. A BTreeMap keeps everything in coverage
    // order from the start.
    let mut groups: std::collections::BTreeMap<u16, Vec<(u16, i16)>> = Default::default();
    for (left, right, value) in &doc.kerning {
        let (Some(l), Some(r)) = (gid_of(left), gid_of(right)) else {
            continue;
        };
        if l == r {
            continue;
        }
        let value = value.round().clamp(-32768.0, 32767.0) as i16;
        groups.entry(l).or_default().push((r, value));
    }
    if groups.is_empty() {
        return None;
    }

    // Coverage order must match the PairSet order, and each PairSet's records
    // must be ordered by second glyph id.
    let coverage: CoverageTable = groups.keys().map(|g| GlyphId16::new(*g)).collect();
    let mut pair_sets: Vec<PairSet> = Vec::new();
    for pairs in groups.values_mut() {
        pairs.sort();
        pairs.dedup();
        let records = pairs
            .iter()
            .map(|(second, value)| {
                let mut value_record1 = ValueRecord::new();
                value_record1.x_advance = Some(*value);
                PairValueRecord::new(GlyphId16::new(*second), value_record1, ValueRecord::new())
            })
            .collect();
        pair_sets.push(PairSet::new(records));
    }

    let lookup = Lookup::new(
        LookupFlag::default(),
        vec![PairPos::Format1(PairPosFormat1::new(coverage, pair_sets))],
    );
    let lookup_list = PositionLookupList::new(vec![PositionLookup::Pair(lookup)]);

    let feature = FeatureRecord::new(Tag::new(b"kern"), Feature::new(None, vec![0]));
    Some(Gpos::new(
        script_list_for(doc, 1),
        FeatureList::new(vec![feature]),
        lookup_list,
    ))
}

/// A ScriptList with one script and a default LangSys enabling every feature.
fn script_list_for(doc: &Document, feature_count: usize) -> ScriptList {
    let mut tag_bytes = [0u8; 4];
    tag_bytes.copy_from_slice(doc.script.id().as_bytes());
    let script_tag = Tag::new(&tag_bytes);
    let lang_sys = LangSys::new((0..feature_count as u16).collect());
    ScriptList::new(vec![ScriptRecord::new(
        script_tag,
        Script::new(Some(lang_sys), vec![]),
    )])
}

#[cfg(test)]
mod tests {
    use super::*;
    use fontmaker_core::document::Script;
    use fontmaker_script as scripts;

    fn gid_of_factory(order: &[String]) -> impl Fn(&str) -> Option<u16> + '_ {
        move |name: &str| order.iter().position(|n| n == name).map(|i| i as u16)
    }

    fn arabic_document() -> Document {
        let mut doc = Document::default();
        scripts::apply_script(&mut doc, Script::Arabic);
        doc
    }

    #[test]
    fn gsub_appears_for_arabic_with_drawn_forms() {
        let mut doc = arabic_document();
        // Draw the medial form of ب: the medi feature then has work to do.
        if let Some(g) = doc.glyph_mut("uni0628.medi") {
            g.contours
                .push(fontmaker_core::stroke::rectangle(0.0, 0.0, 100.0, 100.0));
        }
        let order = doc.glyph_order.clone();
        let drawn = |name: &str| doc.glyph(name).map(|g| g.is_drawn()).unwrap_or(false);
        let gsub = build_gsub(&doc, &drawn, &gid_of_factory(&order)).expect("GSUB built");
        assert_eq!(gsub.feature_list.feature_records.len(), 1);
        assert_eq!(
            gsub.feature_list.feature_records[0].feature_tag,
            Tag::new(b"medi")
        );
        assert_eq!(gsub.lookup_list.lookups.len(), 1);
    }

    #[test]
    fn gsub_skipped_for_latin() {
        let mut doc = Document::default();
        scripts::apply_script(&mut doc, Script::Latin);
        let order = doc.glyph_order.clone();
        let drawn = |name: &str| doc.glyph(name).map(|g| g.is_drawn()).unwrap_or(false);
        assert!(build_gsub(&doc, &drawn, &gid_of_factory(&order)).is_none());
    }

    #[test]
    fn gsub_skipped_when_no_forms_drawn() {
        let doc = arabic_document();
        let order = doc.glyph_order.clone();
        let drawn = |name: &str| doc.glyph(name).map(|g| g.is_drawn()).unwrap_or(false);
        assert!(build_gsub(&doc, &drawn, &gid_of_factory(&order)).is_none());
    }

    #[test]
    fn gpos_groups_pairs_by_left_glyph() {
        let mut doc = arabic_document();
        doc.set_kern("uni0628", "uni0627", -40.0);
        doc.set_kern("uni0628", "uni0645", -30.0);
        doc.set_kern("uni0645", "uni0627", -20.0);
        let order = doc.glyph_order.clone();
        let gpos = build_gpos_kern(&doc, &gid_of_factory(&order)).expect("GPOS built");

        let PositionLookup::Pair(lookup) = &*gpos.lookup_list.lookups[0] else {
            panic!("expected a Pair lookup");
        };
        let PairPos::Format1(format1) = &*lookup.subtables[0] else {
            panic!("expected PairPosFormat1");
        };
        assert_eq!(format1.pair_sets.len(), 2, "two distinct left glyphs");
        let records = &format1.pair_sets[0].pair_value_records;
        assert_eq!(records.len(), 2, "uni0628 has two right partners");
        assert_eq!(records[0].value_record1.x_advance, Some(-40));
        assert_eq!(records[1].value_record1.x_advance, Some(-30));
    }

    #[test]
    fn gpos_skipped_without_kerning() {
        let doc = arabic_document();
        let order = doc.glyph_order.clone();
        assert!(build_gpos_kern(&doc, &gid_of_factory(&order)).is_none());
    }

    #[test]
    fn unknown_glyphs_are_skipped_not_fatal() {
        let mut doc = arabic_document();
        doc.set_kern("uni0628", "ghost-glyph", -40.0);
        let order = doc.glyph_order.clone();
        assert!(
            build_gpos_kern(&doc, &gid_of_factory(&order)).is_none(),
            "the only pair referenced an unknown glyph"
        );
    }

    #[test]
    fn form_pairs_match_joining_rules() {
        // The Arabic charset has 25 dual-joining letters (including the
        // tatweel) and 11 right-joining ones.
        let doc = arabic_document();
        let duals = doc
            .charset
            .iter()
            .filter(|e| e.joining == fontmaker_core::JoiningType::DualJoining)
            .count();
        let rights = doc
            .charset
            .iter()
            .filter(|e| e.joining == fontmaker_core::JoiningType::RightJoining)
            .count();
        assert_eq!(duals, 25, "dual-joining letters");
        assert_eq!(rights, 11, "right-joining letters");
        // init and medi apply to duals only; fina also applies to rights.
        assert_eq!(duals, 25);
        assert_eq!(duals + rights, 36);
    }
}
