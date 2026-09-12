use super::Tabs;
use crate::layout::LayoutOptions;
use std::{
	path::PathBuf,
	sync::Arc,
	time::{Duration, Instant},
};

#[test]
fn switching_restores_sessions_and_requests_remain_globally_ordered() {
	let mut tabs = Tabs::default();
	let now = Instant::now();
	tabs.open(PathBuf::from("a.md"), now);
	let first = tabs.request(LayoutOptions::default(), true).unwrap();
	tabs.session.scroll = 123.0;
	tabs.open(PathBuf::from("b.md"), now);
	let second = tabs.request(LayoutOptions::default(), false).unwrap();
	assert!(second.version > first.version);
	assert!(tabs.select(0, now));
	assert_eq!(tabs.session.scroll, 123.0);
	assert!(tabs.session.follow_update);
	let third = tabs.request(LayoutOptions::default(), false).unwrap();
	assert!(third.version > second.version);
	assert_eq!(third.content_version, first.content_version);
}

#[test]
fn closing_tabs_preserves_active_state_and_releases_only_inactive_documents() {
	let mut tabs = Tabs::default();
	let now = Instant::now();
	tabs.open(PathBuf::from("a.md"), now);
	tabs.session.document = Some(Arc::new(crate::document::parse("a")));
	tabs.open(PathBuf::from("b.md"), now);
	tabs.session.document = Some(Arc::new(crate::document::parse("b")));
	tabs.release_inactive(now + Duration::from_secs(21));
	assert!(tabs.entries()[0].session.document.is_none());
	assert!(tabs.session.document.is_some());
	assert!(matches!(tabs.close(0, now), super::Closed::Inactive));
	assert_eq!(tabs.active(), 0);
	assert_eq!(tabs.session.path, Some(PathBuf::from("b.md")));
	assert!(matches!(tabs.close(0, now), super::Closed::Active));
	assert!(tabs.session.path.is_none());
	assert!(tabs.request(LayoutOptions::default(), false).is_none());
}

#[test]
fn closing_active_tab_restores_neighbor_and_invalid_indices_preserve_state() {
	let mut tabs = Tabs::default();
	let now = Instant::now();
	for (name, scroll) in [("a.md", 10.0), ("b.md", 20.0), ("c.md", 30.0)] {
		tabs.open(PathBuf::from(name), now);
		tabs.session.scroll = scroll;
	}
	assert!(tabs.select(1, now));
	assert!(matches!(tabs.close(1, now), super::Closed::Active));
	assert_eq!(tabs.session.path, Some(PathBuf::from("c.md")));
	assert_eq!(tabs.session.scroll, 30.0);
	assert!(tabs.select(0, now));
	assert_eq!(tabs.session.scroll, 10.0);
	assert!(!tabs.select(9, now));
	assert!(matches!(tabs.close(9, now), super::Closed::Missing));
}
