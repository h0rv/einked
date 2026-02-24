//! Storage and settings abstraction traits.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FileStoreError {
    Io,
}

#[cfg(feature = "std")]
pub trait ReadSeek: std::io::Read + std::io::Seek {}

#[cfg(feature = "std")]
impl<T: std::io::Read + std::io::Seek> ReadSeek for T {}

pub trait FileStore {
    fn list(&self, path: &str, out: &mut dyn FnMut(&str));
    fn is_dir(&self, _path: &str) -> Option<bool> {
        None
    }
    fn read<'a>(&self, path: &str, buf: &'a mut [u8]) -> Result<&'a [u8], FileStoreError>;
    fn exists(&self, path: &str) -> bool;
    #[cfg(feature = "std")]
    fn open_read_seek(&self, _path: &str) -> Result<Box<dyn ReadSeek>, FileStoreError> {
        Err(FileStoreError::Io)
    }
    #[cfg(feature = "std")]
    fn native_path(&self, _path: &str) -> Option<String> {
        None
    }
}

pub trait SettingsStore {
    fn load_raw(&self, key: u8, buf: &mut [u8]) -> usize;
    fn save_raw(&mut self, key: u8, data: &[u8]);
}
