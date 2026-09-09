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
		atomic::{AtomicU64, Ordering},
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
}
pub struct Request {
	pub version: u64,
	pub content_version: u64,
	pub path: PathBuf,
	pub options: LayoutOptions,
	pub requested: Instant,
}
pub struct Update {
	pub version: u64,
	pub path: PathBuf,
	pub result: Result<ReaderSnapshot, String>,
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
	handle: Option<thread::JoinHandle<()>>,
}
impl Worker {
	pub fn new(done: impl Fn(Update) + Send + 'static) -> Self {
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
		let handle = thread::Builder::new()
			.name("markview-layout".into())
			.stack_size(8 * 1024 * 1024)
			.spawn(move || {
				let mut engine = LayoutEngine::new();
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
							inbox = wake.wait(inbox).unwrap();
						}
						if inbox.stopped {
							break;
						}
						inbox.pending.take().unwrap()
					};
					let mut update = Update {
						version: request.version,
						path: request.path.clone(),
						requested: request.requested,
						result: Err(String::new()),
						read_ms: 0.0,
						parse_ms: 0.0,
						layout_ms: 0.0,
					};
					update.result = (|| -> Result<ReaderSnapshot, String> {
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
							if cached.as_ref().is_some_and(|(path, _, _)| {
								path != &request.path
							}) {
								engine.clear_document_cache();
							}
							cached = Some((
								request.path.clone(),
								request.content_version,
								doc.clone(),
							));
							doc
						};
						if current.load(Ordering::Relaxed) != request.version {
							return Err("Superseded".into());
						}
						let start = Instant::now();
						let layout = engine.layout(&document, &request.options);
						update.layout_ms =
							start.elapsed().as_secs_f64() * 1000.0;
						Ok(ReaderSnapshot {
							document,
							layout,
							content_version: request.content_version,
						})
					})();
					if current.load(Ordering::Relaxed) == request.version {
						done(update);
					}
				}
			})
			.expect("start layout worker");
		Self {
			inbox,
			version,
			handle: Some(handle),
		}
	}
	pub fn submit(&self, request: Request) {
		self.version.store(request.version, Ordering::Relaxed);
		let (lock, wake) = &*self.inbox;
		lock.lock().unwrap().pending = Some(request);
		wake.notify_one();
	}
}
impl Drop for Worker {
	fn drop(&mut self) {
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
			});
		}
		loop {
			let update = rx.recv_timeout(Duration::from_secs(5)).unwrap();
			if update.version == 20 {
				assert_eq!(update.result.unwrap().layout.width, 270.0);
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
			})
		};
		submit(1, 1);
		let first = rx
			.recv_timeout(Duration::from_secs(5))
			.unwrap()
			.result
			.unwrap();
		fs::remove_file(&path).unwrap();
		submit(2, 1);
		let reflow = rx.recv_timeout(Duration::from_secs(5)).unwrap();
		assert_eq!(reflow.read_ms, 0.0);
		assert_eq!(reflow.parse_ms, 0.0);
		assert!(Arc::ptr_eq(
			&first.document,
			&reflow.result.unwrap().document
		));
		submit(3, 2);
		assert!(
			rx.recv_timeout(Duration::from_secs(5))
				.unwrap()
				.result
				.is_err()
		);
		fs::write(&path, "Second").unwrap();
		submit(4, 2);
		submit(5, 2);
		loop {
			let update = rx.recv_timeout(Duration::from_secs(5)).unwrap();
			if update.version == 5 {
				assert_eq!(&*update.result.unwrap().document.source, "Second");
				break;
			}
		}
		assert_eq!(&*first.document.source, "First");
	}
}
