use super::GlyphOrigin;

#[test]
fn glyph_texels_align_at_integer_and_fractional_dpi() {
	for scale in [1.0, 1.25, 1.6, 2.0, 3.0] {
		for n in -2000..2000 {
			let x = n as f32 / 37.0;
			let y = n as f32 / 29.0;
			let origin = GlyphOrigin::new(x, y, scale);
			assert_eq!(origin.x.fract(), 0.0);
			assert_eq!(origin.y.fract(), 0.0);
			assert!(origin.phase < 4);
			let raster_x = origin.x + origin.phase as f32 / 4.0;
			assert!((raster_x - x * scale).abs() <= 0.12501);
			assert!((origin.y - y * scale).abs() <= 0.50001);
		}
	}
}

#[test]
fn subpixel_phase_carries_across_pixel_and_zero_boundaries() {
	for (x, expected_x, expected_phase) in [
		(0.99, 1.0, 0),
		(0.74, 0.0, 3),
		(-0.26, -1.0, 3),
		(-0.01, 0.0, 0),
		(-1.01, -1.0, 0),
	] {
		let origin = GlyphOrigin::new(x, 0.0, 1.0);
		assert_eq!(origin.x, expected_x);
		assert_eq!(origin.phase, expected_phase);
	}
}

#[test]
fn whole_pixel_translation_reuses_raster_phase() {
	for phase in 0..4 {
		let x = phase as f32 / 4.0;
		for shift in -10..10 {
			let origin = GlyphOrigin::new(x + shift as f32, 0.0, 1.0);
			assert_eq!(origin.phase, phase);
			assert_eq!(origin.x, shift as f32);
		}
	}
}
