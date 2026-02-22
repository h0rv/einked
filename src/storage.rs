//! Storage and settings abstraction traits.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FileStoreError {
    Io,
}

pub trait FileStore {
    fn list(&self, path: &str, out: &mut dyn FnMut(&str));
    fn read<'a>(&self, path: &str, buf: &'a mut [u8]) -> Result<&'a [u8], FileStoreError>;
    fn exists(&self, path: &str) -> bool;
}

pub trait SettingsStore {
    fn load_raw(&self, key: u8, buf: &mut [u8]) -> usize;
    fn save_raw(&mut self, key: u8, data: &[u8]);
}
