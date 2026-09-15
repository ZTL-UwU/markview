use super::*;
fn shaper() -> TextShaper {
	let mut s = TextShaper::new();
	s.fonts.collection = parley::fontique::Collection::new(
		parley::fontique::CollectionOptions {
			system_fonts: false,
			..Default::default()
		},
	);
	for (family, file) in [
		("Primary", "KaTeX_Main-Italic.ttf"),
		("Fallback", "KaTeX_AMS-Regular.ttf"),
	] {
		let data = ratex_katex_fonts::ttf_bytes(file).unwrap().into_owned();
		s.fonts.collection.register_fonts(
			data.into(),
			Some(parley::fontique::FontInfoOverride {
				family_name: Some(family),
				// KaTeX uses separate slanted outlines but labels them Normal.
				style: Some(if family == "Primary" {
					FontStyle::Italic
				} else {
					FontStyle::Normal
				}),
				..Default::default()
			}),
		);
	}
	let mut style = (*Stylesheet::bundled(false)).clone();
	style.merge(&Stylesheet::parse("format_version=1\nversion=1\n[em]\nfont=[{family='Primary',variant='italic'},{family='Fallback'}]").unwrap());
	s.set_stylesheet(Arc::new(style));
	s
}
#[test]
fn candidates_have_independent_real_faces_and_cluster_coverage() {
	let mut s = shaper();
	let appearance = s.stylesheet.inline(
		&s.appearance,
		&crate::document::TextStyle {
			italic: true,
			..Default::default()
		},
	);
	let latin = s.choose_font("a", &appearance).unwrap();
	assert_eq!(latin.family, "Primary");
	assert_eq!(latin.style, FontStyle::Italic);
	let other = (0x20..0x3000)
		.filter_map(char::from_u32)
		.find(|c| {
			let primary = swash::FontRef::from_index(
				latin.font.data.data(),
				latin.font.index as usize,
			)
			.unwrap();
			primary.charmap().map(*c) == 0
				&& s.choose_font(&c.to_string(), &appearance)
					.is_some_and(|f| f.family == "Fallback")
		})
		.expect("the AMS fixture has symbols absent from Main Italic");
	let fallback = s.choose_font(&other.to_string(), &appearance).unwrap();
	assert_eq!(fallback.family, "Fallback");
	assert_eq!(fallback.style, FontStyle::Normal);
	let text = format!("a{other}");
	let clusters = s.shape(
		&text,
		&[Span {
			range: 0..text.len(),
			style: crate::document::TextStyle {
				italic: true,
				..Default::default()
			},
		}],
		18.,
		false,
	);
	assert!(
		clusters[0]
			.glyphs
			.iter()
			.all(|g| g.font.data.id() == latin.font.data.id())
	);
	assert!(
		clusters
			.last()
			.unwrap()
			.glyphs
			.iter()
			.all(|g| g.font.data.id() == fallback.font.data.id())
	);
	// A candidate must cover the entire combining cluster, never just its base.
	if let Some(face) = s.choose_font("a\u{301}", &appearance) {
		let font = swash::FontRef::from_index(
			face.font.data.data(),
			face.font.index as usize,
		)
		.unwrap();
		assert_ne!(font.charmap().map('a'), 0);
		assert_ne!(font.charmap().map('\u{301}'), 0);
	}
}
#[test]
fn unavailable_variant_weight_and_family_are_skipped() {
	let mut s = shaper();
	let appearance = TextAppearance {
		font: vec![
			Font {
				family: "Missing".into(),
				variant: Variant::Normal,
				weight: None,
			},
			Font {
				family: "Fallback".into(),
				variant: Variant::Italic,
				weight: None,
			},
			Font {
				family: "Primary".into(),
				variant: Variant::Italic,
				weight: Some(700),
			},
			Font {
				family: "Primary".into(),
				variant: Variant::Italic,
				weight: Some(400),
			},
		],
		..Default::default()
	};
	let face = s.choose_font("a", &appearance).unwrap();
	assert_eq!(face.family, "Primary");
	assert_eq!(face.weight, 400);
	assert_eq!(face.style, FontStyle::Italic);
}

#[test]
fn font_choices_are_scoped_and_invalidated_with_stylesheet() {
	let mut s = shaper();
	let appearance = s.stylesheet.inline(
		&s.appearance,
		&TextStyle {
			italic: true,
			..Default::default()
		},
	);
	assert_eq!(s.choose_font("a", &appearance).unwrap().family, "Primary");
	let index = s.resolve_fonts(&appearance);
	assert!(s.font_sets[index].choices.contains_key("a"));
	let missing = "\u{10ffff}";
	assert!(s.choose_font(missing, &appearance).is_none());
	assert_eq!(s.font_sets[index].choices.get(missing), Some(&None));
	let mut other = appearance.clone();
	other.font = vec![Font {
		family: "Fallback".into(),
		variant: Variant::Normal,
		weight: None,
	}];
	assert_ne!(index, s.resolve_fonts(&other));
	let mut style = (*s.stylesheet).clone();
	style.merge(
		&Stylesheet::parse(
			"format_version=1\nversion=1\n[[fontdef]]\nid='Primary'\nlookfor=['Missing']",
		)
		.unwrap(),
	);
	s.set_stylesheet(Arc::new(style));
	assert!(s.font_sets.is_empty());
	assert!(s.faces.is_empty());
	assert_ne!(
		s.choose_font("a", &appearance).map(|f| f.family),
		Some("Primary".into())
	);
}

#[test]
fn font_choice_cache_is_bounded() {
	let mut set = FontSet::default();
	for i in 0..5000 {
		assert_eq!(set.choose(&format!("word{i}")), None);
	}
	assert_eq!(set.choices.len(), 4096);
	let mut set = FontSet::default();
	set.choose(&"a".repeat(129));
	assert!(set.choices.is_empty());
}

#[test]
fn cached_choices_preserve_contextual_shaping() {
	let mut s = shaper();
	for text in [
		"office affine",
		"a\u{301} a\u{200d}",
		"中文 English",
		"مرحبا بالعالم",
		"אבג abc",
		"👩‍💻",
	] {
		for set in &mut s.font_sets {
			set.choices.clear();
		}
		let spans = [Span {
			range: 0..text.len(),
			style: TextStyle {
				italic: true,
				..Default::default()
			},
		}];
		let cold = s.shape(text, &spans, 18., false);
		let warm = s.shape(text, &spans, 18., false);
		assert_eq!(cold.len(), warm.len());
		for (a, b) in cold.iter().zip(&warm) {
			assert_eq!(
				(
					&a.range,
					a.rtl,
					a.width,
					a.ascent,
					a.descent,
					a.continuation
				),
				(
					&b.range,
					b.rtl,
					b.width,
					b.ascent,
					b.descent,
					b.continuation
				)
			);
			assert_eq!(a.glyphs.len(), b.glyphs.len());
			for (a, b) in a.glyphs.iter().zip(&b.glyphs) {
				assert_eq!(
					(
						a.font.data.id(),
						a.font.index,
						a.id,
						a.size,
						a.x,
						a.y,
						&a.coords
					),
					(
						b.font.data.id(),
						b.font.index,
						b.id,
						b.size,
						b.x,
						b.y,
						&b.coords
					)
				);
			}
		}
	}
}
