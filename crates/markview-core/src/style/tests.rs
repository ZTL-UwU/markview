use super::*;
#[test]
fn strict_schema() {
	for bad in [
		"[body]\ncolor='#ffffff'",
		"format_version=2\nversion=1",
		"format_version=1",
		"format_version=1\nversion=1\n[em]\nfont=[]",
		"format_version=1\nversion=1\n[p]\nsize=nan",
		"format_version=1\nversion=1\n[body]\nbackground='#ffffff00'",
		"format_version=1\nversion=1\n[em]\ncolorz='#ffffff'",
		"format_version=1\nversion=1\n[ui]\npadding=2",
		"format_version=1\nversion=1\n[math]\nfont=[{family='serif'}]",
		"format_version=1\nversion=1\n[[fontdef]]\nid='cjk'\ntype='none'\nlookfor=['serif']",
	] {
		assert!(Stylesheet::parse(bad).is_err(), "{bad}");
	}
}
#[test]
fn scrollbar_sizes_are_configurable_and_validated() {
	// A stylesheet without a scrollbar rule falls back to the built-in
	// defaults; the bundled theme is free to pick its own sizes.
	let bare =
		Stylesheet::parse("format_version=1\nversion=1\n[p]\ncolor='#000000'")
			.unwrap();
	assert_eq!(bare.scrollbar_metrics(), ScrollbarMetrics::DOCUMENT);
	assert_eq!(
		bare.overflow_scrollbar_metrics(),
		ScrollbarMetrics::OVERFLOW
	);
	assert_eq!(bare.scrollbar_gutter(), SCROLLBAR_GUTTER);
	let mut sheet = (*Stylesheet::bundled(false)).clone();
	sheet.merge(
		&Stylesheet::parse(
			"format_version=1\nversion=1\n[scrollbar]\nthickness=3.0\nthickness_hover=9.0\noverflow_thickness=4.0\noverflow_thickness_hover=4.0\ngutter=12.0",
		)
		.unwrap(),
	);
	assert_eq!(
		sheet.scrollbar_metrics(),
		ScrollbarMetrics {
			thickness: 3.0,
			thickness_hover: 9.0
		}
	);
	assert_eq!(
		sheet.overflow_scrollbar_metrics(),
		ScrollbarMetrics {
			thickness: 4.0,
			thickness_hover: 4.0
		}
	);
	assert_eq!(sheet.scrollbar_gutter(), 12.0);
	// A theme that only overrides colors keeps the bundled sizes.
	sheet.merge(
		&Stylesheet::parse(
			"format_version=1\nversion=1\n[scrollbar]\nthumb='#000000'",
		)
		.unwrap(),
	);
	assert_eq!(sheet.scrollbar_metrics().thickness, 3.0);
	assert_eq!(sheet.scrollbar_gutter(), 12.0);
	for bad in [
		"format_version=1\nversion=1\n[scrollbar]\nthickness=0.0",
		"format_version=1\nversion=1\n[scrollbar]\nthickness_hover=-1.0",
		"format_version=1\nversion=1\n[scrollbar]\noverflow_thickness=0.0",
		"format_version=1\nversion=1\n[scrollbar]\ngutter=-1.0",
		"format_version=1\nversion=1\n[scrollbar]\ngutter=nan",
		"format_version=1\nversion=1\n[p]\nthickness=4.0",
	] {
		assert!(Stylesheet::parse(bad).is_err(), "{bad}");
	}
}
#[test]
fn fontdefs_are_selected_by_cjk_type() {
	let source = "format_version=1\nversion=1\n[[fontdef]]\nid='cjk'\ntype='SC'\nlookfor=['SC']\n[[fontdef]]\nid='cjk'\ntype='TC'\nlookfor=['TC']";
	let mut sheet = Stylesheet::parse(source).unwrap();
	assert!(!sheet.fontdefs.contains_key("cjk"));
	sheet.set_cjk_type(CjkType::Sc);
	assert_eq!(sheet.fontdefs["cjk"].lookfor, ["SC"]);
	sheet.set_cjk_type(CjkType::Tc);
	assert_eq!(sheet.fontdefs["cjk"].lookfor, ["TC"]);
	sheet.set_cjk_type(CjkType::Jp);
	assert!(!sheet.fontdefs.contains_key("cjk"));
}
#[test]
fn cascade_arrays_and_font_defaults() {
	let mut low=Stylesheet::parse("format_version=1\nversion=1\n[em]\ncolor='#123456'\nfont=[{family='Noto Serif',variant='italic'},{family='落霞文楷'}]").unwrap();
	let high =
		Stylesheet::parse("format_version=1\nversion=2\n[em]\ncolor='#abcdef'")
			.unwrap();
	low.merge(&high);
	assert_eq!(
		low.rule(Role::Em).font.as_ref().unwrap()[1].variant,
		Variant::Normal
	);
	assert_eq!(
		low.color(Role::Em, ColorField::Color),
		Color(0xabcdefff).rgba()
	);
	low.merge(
		&Stylesheet::parse(
			"format_version=1\nversion=3\n[em]\nfont=[{family='serif'}]",
		)
		.unwrap(),
	);
	assert_eq!(low.rule(Role::Em).font.as_ref().unwrap().len(), 1);
}
#[test]
fn colors_do_not_change_layout_identity() {
	let mut s = (*Stylesheet::bundled(false)).clone();
	let k = s.layout_key();
	s.merge(
		&Stylesheet::parse(
			"format_version=1\nversion=2\n[em]\ncolor='#ffffff'",
		)
		.unwrap(),
	);
	assert_eq!(k, s.layout_key());
	s.merge(
		&Stylesheet::parse("format_version=1\nversion=2\n[em]\nsize=1.2")
			.unwrap(),
	);
	assert_ne!(k, s.layout_key());
	assert!(
		Stylesheet::bundled(true)
			.rule(Role::Body)
			.background
			.is_some()
	);
	let hover = Stylesheet::parse(
		"format_version=1\nversion=1\n[link]\ncolor='#123456'\n[link.hover]\ncolor='#abcdef'",
	)
	.unwrap();
	assert_eq!(
		hover.paint(Paint::Styled(Role::LinkHover, ColorField::Color)),
		Color(0xabcdefff).rgba()
	);
}
