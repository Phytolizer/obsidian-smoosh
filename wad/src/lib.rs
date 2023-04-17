use std::collections::HashMap;
use std::fs::File;
use std::io::Cursor;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::path::Path;
use std::string::FromUtf8Error;

use byteorder::LittleEndian;
use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use zip::ZipArchive;

trait FileLike: std::io::Read + std::io::Seek {}
impl<T> FileLike for T where T: Read + Seek {}

#[derive(Debug, thiserror::Error)]
pub enum WadError {
    #[error("failed to read header: {0}")]
    CouldntReadHeader(std::io::Error),
    #[error("failed to write header: {0}")]
    CouldntWriteHeader(std::io::Error),
    #[error("failed to read directory entry: {0}")]
    CouldntReadEntry(std::io::Error),
    #[error("failed to write directory entry: {0}")]
    CouldntWriteEntry(std::io::Error),
    #[error("failed to read lump: {0}")]
    CouldntReadLump(std::io::Error),
    #[error("failed to write lump: {0}")]
    CouldntWriteLump(std::io::Error),
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
    fn new(f: &mut dyn FileLike) -> WadResult<Self> {
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

    fn write(&self, f: &mut File) -> WadResult<()> {
        f.write_all(b"PWAD").map_err(WadError::CouldntWriteHeader)?;
        f.write_i32::<LittleEndian>(self.num_lumps)
            .map_err(WadError::CouldntWriteHeader)?;
        f.write_i32::<LittleEndian>(self.directory_offset)
            .map_err(WadError::CouldntWriteHeader)?;
        Ok(())
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

    pub fn write(&self, f: &mut File) -> WadResult<()> {
        for entry in &self.0 {
            entry.write(f)?;
        }
        Ok(())
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
    fn new(f: &mut dyn FileLike) -> WadResult<Self> {
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

    fn write(&self, f: &mut File) -> WadResult<()> {
        f.write_i32::<LittleEndian>(self.offset)
            .map_err(WadError::CouldntWriteEntry)?;
        f.write_i32::<LittleEndian>(self.size)
            .map_err(WadError::CouldntWriteEntry)?;
        let mut cursor = Cursor::new([0u8; 8]);
        cursor
            .write_all(self.name.as_bytes())
            .map_err(WadError::CouldntWriteEntry)?;
        f.write_all(&cursor.into_inner())
            .map_err(WadError::CouldntWriteEntry)?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Lump {
    pub name: String,
    pub data: Vec<u8>,
}

impl Lump {
    fn new(f: &mut dyn FileLike, entry: &DirectoryEntry) -> Result<Lump, WadError> {
        let mut bytes = vec![0; entry.size as usize];
        f.seek(SeekFrom::Start(entry.offset as u64))
            .map_err(WadError::CouldntReadLump)?;
        f.read_exact(&mut bytes)
            .map_err(WadError::CouldntReadLump)?;

        Ok(Lump {
            name: entry.name.clone(),
            data: bytes,
        })
    }

    fn write(&self, f: &mut File) -> WadResult<()> {
        f.write_all(&self.data)
            .map_err(WadError::CouldntWriteLump)?;
        Ok(())
    }
}

/// A WAD file.
#[derive(Debug)]
pub struct Wad {
    pub directory: Directory,
    pub lumps: Vec<Lump>,
    pub lump_index: HashMap<String, usize>,
}

impl Wad {
    /// Opens a WAD file.
    pub fn new<P>(path: P) -> WadResult<Self>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        let mut f: Box<dyn FileLike> = 'check_and_unzip: {
            let f = File::open(path).map_err(WadError::CouldntReadHeader)?;
            'check_zip: {
                if let Ok(mut archive) = ZipArchive::new(f) {
                    let wadname = 'find_wad: {
                        for name in archive.file_names() {
                            if name.ends_with(".wad") {
                                break 'find_wad name.to_string();
                            }
                        }
                        break 'check_zip;
                    };
                    let mut bytes = Vec::new();
                    archive
                        .by_name(&wadname)
                        .unwrap()
                        .read_to_end(&mut bytes)
                        .map_err(WadError::CouldntReadHeader)?;
                    break 'check_and_unzip Box::new(Cursor::new(bytes));
                }
            }
            Box::new(File::open(path).unwrap())
        };
        let header = WadHeader::new(f.as_mut())?;
        let mut directory = Vec::with_capacity(header.num_lumps as usize);
        f.seek(SeekFrom::Start(header.directory_offset as u64))
            .map_err(WadError::CouldntReadHeader)?;
        for _ in 0..header.num_lumps {
            directory.push(DirectoryEntry::new(f.as_mut())?);
        }

        let mut lumps = Vec::with_capacity(header.num_lumps as usize);
        let mut lump_index = HashMap::new();
        for entry in &directory {
            lump_index.insert(entry.name.clone(), lumps.len());
            lumps.push(Lump::new(&mut f, entry)?);
        }

        Ok(Wad {
            directory: Directory(directory),
            lumps,
            lump_index,
        })
    }

    pub fn write<P: AsRef<Path>>(&self, path: P) -> WadResult<()> {
        let path = path.as_ref();
        let mut f = File::create(path).map_err(WadError::CouldntWriteHeader)?;
        let header = WadHeader {
            num_lumps: self.directory.0.len() as i32,
            directory_offset: 12,
        };
        header.write(&mut f)?;

        let mut offset = 12 + self.directory.0.len() * 16;
        for lump in &self.lumps {
            let entry = DirectoryEntry {
                offset: offset.try_into().unwrap(),
                size: lump.data.len() as i32,
                name: lump.name.clone(),
            };
            entry.write(&mut f)?;
            offset += lump.data.len();
        }

        for lump in &self.lumps {
            lump.write(&mut f)?;
        }

        Ok(())
    }
}
