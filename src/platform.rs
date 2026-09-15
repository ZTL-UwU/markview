//! OS effects stay outside the reading core.
#[derive(Default)]
pub struct Clipboard {
	inner: Option<arboard::Clipboard>,
}
impl Clipboard {
	pub fn read(&mut self) -> anyhow::Result<String> {
		if self.inner.is_none() {
			self.inner = Some(arboard::Clipboard::new()?);
		}
		Ok(self.inner.as_mut().unwrap().get_text()?)
	}

	pub fn write(&mut self, text: String) -> anyhow::Result<()> {
		if self.inner.is_none() {
			self.inner = Some(arboard::Clipboard::new()?);
		}
		self.inner.as_mut().unwrap().set_text(text)?;
		Ok(())
	}
}
