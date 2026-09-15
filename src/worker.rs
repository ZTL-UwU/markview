//! Latest-request-wins document processing, independent of file observation.
use crate::{
	document,
	file::read_document,
	layout::{LayoutEngine, LayoutOptions, LayoutSnapshot},
};
use std::{
	path::PathBuf,
	sync::{
		Arc, Condvar, Mutex,
		atomic::{AtomicU32, AtomicU64, Ordering},
	},
	thread,
	time::Instant,
};
/// Text and geometry are accepted together by the UI.
#[derive(Clone, Debug)]
pub struct ReaderSnapshot {
	pub document: Arc<document::Document>,
	pub layout: LayoutSnapshot,
	pub content_version: u64,
	pub complete: bool,
}
#[derive(Clone)]
pub struct Request {
	pub version: u64,
	pub content_version: u64,
	pub path: PathBuf,
	pub options: LayoutOptions,
	pub requested: Instant,
	/// Initial viewport bottom plus prefetch, in document coordinates.
	pub coverage: f32,
}
#[derive(Clone)]
pub struct Update {
	pub version: u64,
	pub path: PathBuf,
	pub result: Option<Result<ReaderSnapshot, String>>,
	pub requested: Instant,
	pub read_ms: f64,
	pub parse_ms: f64,
	pub layout_ms: f64,
}

struct Inbox {
	pending: Option<Request>,
	stopped: bool,
}
pub struct Worker {
	inbox: Arc<(Mutex<Inbox>, Condvar)>,
	version: Arc<AtomicU64>,
	coverage: Arc<AtomicU32>,
	handle: Option<thread::JoinHandle<()>>,
}
impl Worker {
	#[cfg(test)]
	pub fn new(done: impl Fn(Update) + Send + 'static) -> Self {
		Self::with_images(true, done)
	}
	pub fn with_images(
		offline: bool,
		done: impl Fn(Update) + Send + 'static,
	) -> Self {
		let inbox = Arc::new((
			Mutex::new(Inbox {
				pending: None,
				stopped: false,
			}),
			Condvar::new(),
		));
		let thread_inbox = inbox.clone();
		let version = Arc::new(AtomicU64::new(0));
		let current = version.clone();
		let coverage = Arc::new(AtomicU32::new(f32::INFINITY.to_bits()));
		let target = coverage.clone();
		let handle = thread::Builder::new()
			.name("markview-layout".into())
			.stack_size(8 * 1024 * 1024)
			.spawn(move || {
				let mut engine = LayoutEngine::new();
				let mut images = crate::images::Images::new(offline);
				let mut last: Option<Request> = None;
				let mut completed_version = 0;
				let mut cached: Option<(
					PathBuf,
					u64,
					Arc<document::Document>,
				)> = None;
				loop {
					let request = {
						let (lock, wake) = &*thread_inbox;
						let mut inbox = lock.lock().unwrap();
						while inbox.pending.is_none() && !inbox.stopped {
							inbox = wake
								.wait_timeout(
									inbox,
									std::time::Duration::from_millis(50),
								)
								.unwrap()
								.0;
							if inbox.pending.is_none() && images.poll() {
								inbox.pending = last.clone();
							}
							if inbox.pending.is_none()
								&& last.is_some() && engine.poll_highlights()
							{
								inbox.pending = last.clone();
							}
						}
						if inbox.stopped {
							break;
						}
						inbox.pending.take().unwrap()
					};
					last = Some(request.clone());
					let mut update = Update {
						version: request.version,
						path: request.path.clone(),
						requested: request.requested,
						result: None,
						read_ms: 0.0,
						parse_ms: 0.0,
						layout_ms: 0.0,
					};
					update.result =
						Some((|| -> Result<ReaderSnapshot, String> {
							let document = if let Some((path, revision, doc)) =
								&cached && path == &request.path
								&& *revision == request.content_version
							{
								doc.clone()
							} else {
								let start = Instant::now();
								let text = read_document(&request.path)
									.map_err(|e| format!("{e:#}"))?;
								update.read_ms =
									start.elapsed().as_secs_f64() * 1000.0;
								let start = Instant::now();
								let doc = Arc::new(document::parse(text));
								update.parse_ms =
									start.elapsed().as_secs_f64() * 1000.0;
								if cached.as_ref().is_some_and(
									|(path, _, _)| path != &request.path,
								) {
									engine.clear_document_cache();
								}
								cached = Some((
									request.path.clone(),
									request.content_version,
									doc.clone(),
								));
								doc
							};
							if current.load(Ordering::Relaxed)
								!= request.version
							{
								return Err("Superseded".into());
							}
							let start = Instant::now();
							engine
								.validate_stylesheet(
									&request.options.stylesheet,
								)
								.map_err(|e| format!("Fonts: {e:#}"))?;
							images.prepare(
								&document,
								&request.path,
								request.content_version,
							);
							let mut published_blocks = 0;
							// Small documents finish within a few frames; avoid extra
							// snapshots and redraws when there is little work to hide.
							let progressive =
								document.source.len() >= 32 * 1024;
							let mut published_target = -1.0;
							let mut published_at = Instant::now();
							let layout = engine
								.layout_progressive(
									&document,
									&request.options,
									&images.snapshot,
									|prefix| {
										if current.load(Ordering::Relaxed)
											!= request.version
										{
											return false;
										}
										let wanted = f32::from_bits(
											target.load(Ordering::Relaxed),
										);
										let ready = prefix.height >= wanted
											&& !prefix.blocks.is_empty();
										let demand = wanted != published_target;
										// Geometric batching bounds prefix-copy work on very long files.
										let batch = prefix.blocks.len()
											>= (published_blocks * 2).max(1)
											&& published_at
												.elapsed()
												.as_millis() >= 32;
										if progressive
											&& request.version
												!= completed_version && ready
											&& (demand || batch)
										{
											let mut partial = update.clone();
											partial.layout_ms =
												start.elapsed().as_secs_f64()
													* 1000.;
											partial.result =
												Some(Ok(ReaderSnapshot {
													document: document.clone(),
													layout: prefix.clone(),
													content_version: request
														.content_version,
													complete: false,
												}));
											done(partial);
											published_blocks =
												prefix.blocks.len();
											published_target = wanted;
											published_at = Instant::now();
										}
										true
									},
								)
								.ok_or_else(|| "Superseded".to_string())?;
							update.layout_ms =
								start.elapsed().as_secs_f64() * 1000.0;
							Ok(ReaderSnapshot {
								document,
								layout,
								content_version: request.content_version,
								complete: true,
							})
						})());
					if current.load(Ordering::Relaxed) == request.version {
						if update.result.as_ref().is_some_and(|r| r.is_ok()) {
							completed_version = request.version;
						}
						done(update);
					}
				}
			})
			.expect("start layout worker");
		Self {
			inbox,
			version,
			coverage,
			handle: Some(handle),
		}
	}
	pub fn submit(&self, request: Request) {
		self.coverage
			.store(request.coverage.to_bits(), Ordering::Relaxed);
		self.version.store(request.version, Ordering::Relaxed);
		let (lock, wake) = &*self.inbox;
		lock.lock().unwrap().pending = Some(request);
		wake.notify_one();
	}
	pub fn prioritize(&self, coverage: f32) {
		self.coverage.store(coverage.to_bits(), Ordering::Relaxed);
	}
	pub fn cancel(&self) {
		self.version.store(0, Ordering::Relaxed);
		let (lock, _) = &*self.inbox;
		lock.lock().unwrap().pending = None;
	}
}
impl Drop for Worker {
	fn drop(&mut self) {
		self.version.store(0, Ordering::Relaxed);
		let (lock, wake) = &*self.inbox;
		lock.lock().unwrap().stopped = true;
		wake.notify_one();
		if let Some(t) = self.handle.take() {
			let _ = t.join();
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::{fs, sync::mpsc, time::Duration};
	#[test]
	fn progressive_worker_chases_target_and_cancels_changed_file() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("read.md");
		fs::write(&path, "A paragraph 中文 with **text**.\n\n".repeat(1000))
			.unwrap();
		let (tx, rx) = mpsc::channel();
		let (resume, gate) = mpsc::channel();
		let gate = Mutex::new(gate);
		let paused = AtomicU64::new(0);
		let worker = Worker::new(move |u| {
			let pause =
				u.version == 1 && paused.fetch_add(1, Ordering::Relaxed) < 2;
			tx.send(u).unwrap();
			if pause {
				gate.lock()
					.unwrap()
					.recv_timeout(Duration::from_secs(10))
					.unwrap();
			}
		});
		let request = Request {
			version: 1,
			content_version: 1,
			path: path.clone(),
			options: LayoutOptions::default(),
			requested: Instant::now(),
			coverage: 600.,
		};
		worker.submit(request.clone());
		let first = rx
			.recv_timeout(Duration::from_secs(10))
			.unwrap()
			.result
			.unwrap()
			.unwrap();
		assert!(!first.complete);
		assert!(first.layout.height >= 600.);
		assert!(first.layout.blocks.len() < first.document.blocks.len());
		worker.prioritize(6000.);
		resume.send(()).unwrap();
		let next = rx
			.recv_timeout(Duration::from_secs(10))
			.unwrap()
			.result
			.unwrap()
			.unwrap();
		assert!(!next.complete);
		assert!(next.layout.height >= 6000.);
		assert!(Arc::ptr_eq(
			&first.layout.blocks[0].layout,
			&next.layout.blocks[0].layout
		));
		fs::write(&path, "Replacement").unwrap();
		worker.submit(Request {
			version: 2,
			content_version: 2,
			..request
		});
		resume.send(()).unwrap();
		let update = rx.recv_timeout(Duration::from_secs(10)).unwrap();
		assert_eq!(update.version, 2);
		let final_reader = update.result.unwrap().unwrap();
		assert!(final_reader.complete);
		assert_eq!(&*final_reader.document.source, "Replacement");
		assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
	}

	#[test]
	fn progressive_worker_finishes_with_shared_prefix() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("read.md");
		fs::write(
			&path,
			format!("{}\n\n", "Paragraph. ".repeat(40)).repeat(100),
		)
		.unwrap();
		let (tx, rx) = mpsc::channel();
		let worker = Worker::new(move |u| {
			tx.send(u).unwrap();
		});
		worker.submit(Request {
			version: 1,
			content_version: 1,
			path,
			options: LayoutOptions::default(),
			requested: Instant::now(),
			coverage: 600.,
		});
		let first = rx
			.recv_timeout(Duration::from_secs(10))
			.unwrap()
			.result
			.unwrap()
			.unwrap();
		assert!(!first.complete);
		loop {
			let reader = rx
				.recv_timeout(Duration::from_secs(10))
				.unwrap()
				.result
				.unwrap()
				.unwrap();
			if reader.complete {
				assert_eq!(reader.layout.blocks.len(), 100);
				assert!(Arc::ptr_eq(
					&first.layout.blocks[0].layout,
					&reader.layout.blocks[0].layout
				));
				break;
			}
		}
	}
	#[test]
	fn worker_publishes_latest_request() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("read.md");
		fs::write(&path, "A paragraph.").unwrap();
		let (tx, rx) = mpsc::channel();
		let worker = Worker::new(move |u| {
			let _ = tx.send(u);
		});
		for version in 1..=20 {
			worker.submit(Request {
				version,
				content_version: 1,
				path: path.clone(),
				options: LayoutOptions {
					width: 250.0 + version as f32,
					..Default::default()
				},
				requested: Instant::now(),
				coverage: f32::INFINITY,
			});
		}
		loop {
			let update = rx.recv_timeout(Duration::from_secs(5)).unwrap();
			if update.version == 20 {
				assert_eq!(update.result.unwrap().unwrap().layout.width, 270.0);
				break;
			}
		}
		assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
	}
}

#[cfg(test)]
mod reflow_tests {
	use super::*;
	use std::{fs, sync::mpsc, time::Duration};
	#[test]
	fn reflow_reuses_document_without_reading_and_reload_is_not_lost() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("read.md");
		fs::write(&path, "First").unwrap();
		let (tx, rx) = mpsc::channel();
		let worker = Worker::new(move |u| {
			tx.send(u).unwrap();
		});
		let submit = |version, content_version| {
			worker.submit(Request {
				version,
				content_version,
				path: path.clone(),
				options: LayoutOptions::default(),
				requested: Instant::now(),
				coverage: f32::INFINITY,
			})
		};
		submit(1, 1);
		let first = rx
			.recv_timeout(Duration::from_secs(5))
			.unwrap()
			.result
			.unwrap()
			.unwrap();
		fs::remove_file(&path).unwrap();
		submit(2, 1);
		let reflow = rx.recv_timeout(Duration::from_secs(5)).unwrap();
		assert_eq!(reflow.read_ms, 0.0);
		assert_eq!(reflow.parse_ms, 0.0);
		assert!(Arc::ptr_eq(
			&first.document,
			&reflow.result.unwrap().unwrap().document
		));
		submit(3, 2);
		assert!(
			rx.recv_timeout(Duration::from_secs(5))
				.unwrap()
				.result
				.unwrap()
				.is_err()
		);
		fs::write(&path, "Second").unwrap();
		submit(4, 2);
		submit(5, 2);
		loop {
			let update = rx.recv_timeout(Duration::from_secs(5)).unwrap();
			if update.version == 5 {
				assert_eq!(
					&*update.result.unwrap().unwrap().document.source,
					"Second"
				);
				break;
			}
		}
		assert_eq!(&*first.document.source, "First");
	}
}
