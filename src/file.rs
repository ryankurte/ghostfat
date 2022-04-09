
use crate::ASCII_SPACE;

pub struct File<'a> {
    pub(crate) name: [u8; 11],
    pub(crate) data: FileContent<'a>,
}

pub enum FileContent<'a> {
    Read(&'a [u8]),
    Write(&'a mut [u8]),
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

impl <'a> File<'a> {
    pub fn new<D: Into<FileContent<'a>>>(name: &str, data: D) -> Result<Self, ()> {

        let mut f = Self {
            name: [ASCII_SPACE; 11],
            data: data.into(),
        };

        // Copy name
        let n = name.as_bytes();
        f.name[..n.len()].copy_from_slice(n);

        Ok(f)
    }

    pub fn len(&self) -> usize {
        self.data().len()
    }

    pub fn data(&self) -> &[u8] {
        match &self.data {
            FileContent::Read(r) => r,
            FileContent::Write(w) => w,
        }
    }
}
