use std::fs::File;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::Path;
use std::string::FromUtf8Error;

use byteorder::LittleEndian;
use byteorder::ReadBytesExt;

#[derive(Debug, thiserror::Error)]
pub enum WadError {
    #[error("failed to read header: {0}")]
    CouldntReadHeader(std::io::Error),
    #[error("failed to read directory entry: {0}")]
    CouldntReadEntry(std::io::Error),
    #[error("failed to read lump: {0}")]
    CouldntReadLump(std::io::Error),
    #[error("invalid magic number: {0:?}")]
    InvalidMagicNumber([u8; 4]),
    #[error("invalid lump name: {0}")]
    InvalidLumpName(FromUtf8Error),
    #[error("trailing bytes")]
    TrailingBytes,
    #[error("early EOF")]
    UnexpectedEof,
    #[error("{0}")]
    Other(String),
}

type WadResult<T> = Result<T, WadError>;

/// The header of a WAD file. Contains overview information about the file.
#[derive(Debug)]
struct WadHeader {
    /// The number of lumps in the WAD.
    num_lumps: i32,
    /// The offset to the start of the directory.
    directory_offset: i32,
}

impl WadHeader {
    fn new(f: &mut File) -> WadResult<Self> {
        let mut identification = [0; 4];
        f.read_exact(&mut identification)
            .map_err(WadError::CouldntReadHeader)?;
        if ![b"IWAD", b"PWAD"].contains(&&identification) {
            return Err(WadError::InvalidMagicNumber(identification));
        }
        let num_lumps = f
            .read_i32::<LittleEndian>()
            .map_err(WadError::CouldntReadHeader)?;
        let directory_offset = f
            .read_i32::<LittleEndian>()
            .map_err(WadError::CouldntReadHeader)?;
        Ok(WadHeader {
            num_lumps,
            directory_offset,
        })
    }
}

#[derive(Debug)]
pub struct Directory(Vec<DirectoryEntry>);

impl Directory {
    pub fn iter(&self) -> DirectoryIter {
        DirectoryIter {
            inner: self.0.iter(),
        }
    }
}

pub struct DirectoryIter<'a> {
    inner: std::slice::Iter<'a, DirectoryEntry>,
}

impl<'a> Iterator for DirectoryIter<'a> {
    type Item = &'a DirectoryEntry;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

#[derive(Debug)]
pub struct DirectoryEntry {
    pub offset: i32,
    pub size: i32,
    pub name: String,
}

impl DirectoryEntry {
    fn new(f: &mut File) -> WadResult<Self> {
        let offset = f
            .read_i32::<LittleEndian>()
            .map_err(WadError::CouldntReadEntry)?;
        let size = f
            .read_i32::<LittleEndian>()
            .map_err(WadError::CouldntReadEntry)?;
        let mut name = [0; 8];
        f.read_exact(&mut name)
            .map_err(WadError::CouldntReadEntry)?;
        let name = String::from_utf8(name.into_iter().filter(|c| *c != b'\0').collect())
            .map_err(WadError::InvalidLumpName)?;
        Ok(DirectoryEntry { offset, size, name })
    }
}

#[derive(Debug)]
pub struct Lump(Vec<u8>);

impl Lump {
    fn new(f: &mut File, entry: &DirectoryEntry) -> Result<Lump, WadError> {
        let mut bytes = vec![0; entry.size as usize];
        f.seek(SeekFrom::Start(entry.offset as u64))
            .map_err(WadError::CouldntReadLump)?;
        f.read_exact(&mut bytes)
            .map_err(WadError::CouldntReadLump)?;

        Ok(Lump(bytes))
    }
}

/// A WAD file.
#[derive(Debug)]
pub struct Wad {
    pub directory: Directory,
    pub lumps: Vec<Lump>,
}

impl Wad {
    /// Opens a WAD file.
    pub fn new<P>(path: P) -> WadResult<Self>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        let mut f = File::open(path).map_err(WadError::CouldntReadHeader)?;

        let header = WadHeader::new(&mut f)?;
        let mut directory = Vec::with_capacity(header.num_lumps as usize);
        f.seek(SeekFrom::Start(header.directory_offset as u64))
            .map_err(WadError::CouldntReadHeader)?;
        for _ in 0..header.num_lumps {
            directory.push(DirectoryEntry::new(&mut f)?);
        }

        let mut lumps = Vec::with_capacity(header.num_lumps as usize);
        for entry in &directory {
            lumps.push(Lump::new(&mut f, entry)?);
        }

        Ok(Wad {
            directory: Directory(directory),
            lumps,
        })
    }
}
