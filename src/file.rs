
use crate::ASCII_SPACE;

pub struct File<'a> {
    pub(crate) name: &'a str,
    pub(crate) data: FileContent<'a>,
}

pub enum FileContent<'a> {
    Read(&'a [u8]),
    Write(&'a mut [u8]),
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum FileError {
    InvalidName,
}

bitflags::bitflags! {
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

impl <'a>From<&'a [u8]> for FileContent<'a> {
    fn from(d: &'a [u8]) -> Self {
        FileContent::Read(d)
    }
}

impl <'a, const N: usize>From<&'a [u8; N]> for FileContent<'a> {
    fn from(d: &'a [u8; N]) -> Self {
        FileContent::Read(d.as_ref())
    }
}

impl <'a>From<&'a mut [u8]> for FileContent<'a> {
    fn from(d: &'a mut [u8]) -> Self {
        FileContent::Write(d)
    }
}

impl <'a, const N: usize>From<&'a mut [u8; N]> for FileContent<'a> {
    fn from(d: &'a mut [u8; N]) -> Self {
        FileContent::Write(d.as_mut())
    }
}

impl <'a> File<'a> {
    pub fn new<D: Into<FileContent<'a>>>(name: &'a str, data: D) -> Result<Self, FileError> {

        // TODO: split name and fit to FAT format
        let mut f = Self {
            name,
            data: data.into(),
        };

        // Process file name to check validity
        f.name_fat16_short()?;

        Ok(f)
    }

    pub fn name(&self) -> &str {
        self.name
    }

    pub fn name_fat16_short(&self) -> Result<[u8; 11], FileError> {
        // Split by extension
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
        let mut name = [ASCII_SPACE; 11];
        name[..prefix.len()].copy_from_slice(prefix.as_bytes());
        name[11 - ext.len()..].copy_from_slice(ext.as_bytes());

        Ok(name)
    }

    pub fn len(&self) -> usize {
        self.data().len()
    }

    pub fn attrs(&self) -> Attrs {
        match &self.data {
            FileContent::Read(_) => Attrs::READ_ONLY,
            FileContent::Write(_) => Attrs::empty(),
        }
    }

    pub fn data(&self) -> &[u8] {
        match &self.data {
            FileContent::Read(r) => r,
            FileContent::Write(w) => w,
        }
    }

    pub fn data_mut(&mut self) -> Option<&mut [u8]> {
        match &mut self.data {
            FileContent::Read(_r) => None,
            FileContent::Write(w) => Some(w),
        }
    }
}
