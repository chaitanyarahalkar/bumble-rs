use crate::{H4Transport, Result};
use std::fs::{File, OpenOptions};
use std::path::Path;

/// Bidirectional H4 transport over a file, PTY, or Unix character device.
pub type FileTransport = H4Transport<File>;

impl FileTransport {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        Ok(Self::new(file))
    }

    /// Duplicate the underlying descriptor into independently owned packet
    /// source and sink halves.
    pub fn try_split(self) -> Result<(Self, Self)> {
        let source = Self::new(self.get_ref().try_clone()?);
        Ok((source, self))
    }
}
