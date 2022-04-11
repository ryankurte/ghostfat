
use crate::ASCII_SPACE;

/// Virtual file object
pub struct File<'a, const BLOCK_SIZE: usize = 512> {
    pub(crate) name: &'a str,
    pub(crate) data: FileContent<'a, BLOCK_SIZE>,
}

/// Files may contain a read buffer, write buffer, or read/write trait
pub enum FileContent<'a, const BLOCK_SIZE: usize = 512> {
    /// Read only buffer
    Read(&'a [u8]),
    /// Read/write buffer
    Write(&'a mut [u8]),
    /// Read/write object
    Dynamic(&'a dyn DynamicFile<BLOCK_SIZE>),
}

/// ReadWrite trait for generic file objects
pub trait DynamicFile<const BLOCK_SIZE: usize = 512>: Sync {
    /// Return the maximum length of the virtual vile
    fn len(&self) -> usize;

    /// Read a chunk of the virtual file, returning the read length
    fn read_chunk(&self, index: usize, buff: &mut [u8]) -> usize;

    /// Write a chunk of the virtual file, returning the write length
    fn write_chunk(&self, index: usize, data: &[u8]) -> usize;
}

/// File error types
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum FileError {
    InvalidName,
}

bitflags::bitflags! {
    /// FAT16 file attributes
    pub struct Attrs: u8 {
        const READ_ONLY = 0x01;
        const HIDDEN = 0x02;
        const SYSTEM = 0x04;
        const VOLUME_LABEL=0x08;
        const SUBDIR = 0x10;
        const ARCHIVE = 0x20;
        const DEVICE = 0x40;
    }
}

/// Create a file from an immutable buffer
impl <'a, const BLOCK_SIZE: usize>From<&'a [u8]> for FileContent<'a, BLOCK_SIZE> {
    fn from(d: &'a [u8]) -> Self {
        FileContent::Read(d)
    }
}

/// Create a file from an immutable array
impl <'a, const BLOCK_SIZE: usize, const N: usize>From<&'a [u8; N]> for FileContent<'a, BLOCK_SIZE> {
    fn from(d: &'a [u8; N]) -> Self {
        FileContent::Read(d.as_ref())
    }
}

/// Create a file from a mutable buffer
impl <'a, const BLOCK_SIZE: usize>From<&'a mut [u8]> for FileContent<'a, BLOCK_SIZE> {
    fn from(d: &'a mut [u8]) -> Self {
        FileContent::Write(d)
    }
}

/// Create a file from a mutable array
impl <'a, const BLOCK_SIZE: usize, const N: usize>From<&'a mut [u8; N]> for FileContent<'a, BLOCK_SIZE> {
    fn from(d: &'a mut [u8; N]) -> Self {
        FileContent::Write(d.as_mut())
    }
}

impl <'a, const BLOCK_SIZE: usize> File<'a, BLOCK_SIZE> {
    /// Create a new File object with the provided data
    pub fn new<D: Into<FileContent<'a, BLOCK_SIZE>>>(name: &'a str, data: D) -> Result<Self, FileError> {

        // Build object
        let f = Self {
            name,
            data: data.into(),
        };

        // Check short name generation
        f.short_name()?;

        Ok(f)
    }

    /// Constant helper to create read only files.
    /// 
    /// Beware this function will not check short file name creation
    pub const fn new_ro(name: &'a str, data: &'a [u8]) -> Self {
        Self{ name, data: FileContent::Read(data) }
    }

    /// Constant helper to create read-write files.
    /// 
    /// Beware this function will not check short file name creation
    #[cfg(feature="nightly")]
    pub const fn new_rw(name: &'a str, data: &'a mut [u8]) -> Self {
        Self{ name, data: FileContent::Write(data) }
    }

    /// Constant helper to create dynamic files.
    /// 
    /// Beware this function will not check short file name creation
    pub const fn new_dyn(name: &'a str, data: &'a dyn DynamicFile<BLOCK_SIZE>) -> Self {
        Self{ name, data: FileContent::Dynamic(data) }
    }

    /// Fetch the file name
    pub fn name(&self) -> &str {
        self.name
    }

    /// Fetch short file name for directory entry
    pub(crate) fn short_name(&self) -> Result<[u8; 11], FileError> {
        // Split name by extension
        let mut n = self.name.split(".");
        let (prefix, ext) = match (n.next(), n.next()) {
            (Some(p), Some(e)) => (p, e),
            _ => return Err(FileError::InvalidName),
        };

        // Check prefix and extension will fit FAT buffer
        // TODO: long file names?
        if prefix.len() + ext.len() > 11 {
            return Err(FileError::InvalidName);
        }

        // Copy name
        let mut short_name = [ASCII_SPACE; 11];
        short_name[..prefix.len()].copy_from_slice(prefix.as_bytes());
        short_name[11 - ext.len()..].copy_from_slice(ext.as_bytes());

        Ok(short_name)
    }

    /// Fetch the file length
    pub fn len(&self) -> usize {
        match &self.data {
            FileContent::Read(r) => r.len(),
            FileContent::Write(w) => w.len(),
            FileContent::Dynamic(rw) => rw.len(),
        }
    }

    /// Fetch file attributes
    pub(crate) fn attrs(&self) -> Attrs {
        match &self.data {
            FileContent::Read(_r) => Attrs::READ_ONLY,
            FileContent::Write(_w) => Attrs::empty(),
            FileContent::Dynamic(_rw) => Attrs::empty(),
        }
    }

    /// Read a <= BLOCK_SIZE chunk of the file into the provided buffer
    pub(crate) fn chunk(&self, index: usize, buff: &mut [u8]) -> usize {
        if let FileContent::Dynamic(rw) = &self.data {
            return rw.read_chunk(index, buff)
        }

        let d = match &self.data {
            FileContent::Read(r) => r.chunks(BLOCK_SIZE).nth(index),
            FileContent::Write(w) => w.chunks(BLOCK_SIZE).nth(index),
            _ => unreachable!(),
        };

        if let Some(d) = d {
            let len = usize::min(buff.len(), d.len());
            buff[..len].copy_from_slice(&d[..len]);
            return len;
        }

        return 0;
    }

    /// Write a <= BLOCK_SIZE mutable chunk of the file from the provided buffer
    pub(crate) fn chunk_mut(&mut self, index: usize, data: &[u8]) -> usize {
        match &mut self.data {
            FileContent::Read(_r) => return 0,
            FileContent::Write(w) => {
                if let Some(b) = w.chunks_mut(BLOCK_SIZE).nth(index) {
                    let len = usize::min(b.len(), data.len());
                    b[..len].copy_from_slice(&data[..len]);
                    return len;
                }
            },
            FileContent::Dynamic(rw) => return rw.write_chunk(index, data),
        }

        return 0
    } 
}
