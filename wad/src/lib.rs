use std::fs::File;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::Path;
use std::string::FromUtf8Error;

use serde::de::Visitor;
use serde::Deserialize;

mod wad_de;

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

impl serde::de::Error for WadError {
    fn custom<T>(msg: T) -> Self
    where
        T: std::fmt::Display,
    {
        WadError::Other(msg.to_string())
    }
}

type WadResult<T> = Result<T, WadError>;

/// The header of a WAD file. Contains overview information about the file.
struct WadHeader {
    /// The magic number, "IWAD" or "PWAD".
    identification: [u8; 4],
    /// The number of lumps in the WAD.
    num_lumps: i32,
    /// The offset to the start of the directory.
    directory_offset: i32,
}

impl<'de> Deserialize<'de> for WadHeader {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct WadHeaderVisitor;

        impl<'de> Visitor<'de> for WadHeaderVisitor {
            type Value = WadHeader;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a WAD header")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let identification = [
                    seq.next_element()?
                        .ok_or_else(|| serde::de::Error::invalid_length(0, &self))?,
                    seq.next_element()?
                        .ok_or_else(|| serde::de::Error::invalid_length(1, &self))?,
                    seq.next_element()?
                        .ok_or_else(|| serde::de::Error::invalid_length(2, &self))?,
                    seq.next_element()?
                        .ok_or_else(|| serde::de::Error::invalid_length(3, &self))?,
                ];
                let num_lumps = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(4, &self))?;
                let directory_offset = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(5, &self))?;
                Ok(WadHeader {
                    identification,
                    num_lumps,
                    directory_offset,
                })
            }
        }

        const FIELDS: &[&str] = &[&"identification", &"num_lumps", &"directory_offset"];
        deserializer.deserialize_struct("WadHeader", FIELDS, WadHeaderVisitor)
    }
}

struct Directory(Vec<DirectoryEntry>);

struct DirectoryEntry {
    offset: i32,
    size: i32,
    name: String,
}

impl<'de> Deserialize<'de> for DirectoryEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct DirectoryEntryVisitor;

        impl<'de> Visitor<'de> for DirectoryEntryVisitor {
            type Value = DirectoryEntry;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a WAD directory entry")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let offset = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(0, &self))?;
                let size = seq
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(1, &self))?;
                let name = seq
                    .next_element::<u64>()?
                    .ok_or_else(|| serde::de::Error::invalid_length(2, &self))?;
                let bytes = name.to_le_bytes();
                let name = String::from_utf8(bytes.to_vec()).map_err(serde::de::Error::custom)?;
                Ok(DirectoryEntry { offset, size, name })
            }
        }

        const FIELDS: &[&str] = &[&"offset", &"size", &"name"];
        deserializer.deserialize_struct("DirectoryEntry", FIELDS, DirectoryEntryVisitor)
    }
}

struct Lump(Vec<u8>);

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
struct Wad {
    header: WadHeader,
    directory: Directory,
}

impl Wad {
    /// Opens a WAD file.
    fn open(path: &Path) -> Result<Wad, WadError> {
        let mut f = File::open(path).map_err(WadError::CouldntReadHeader)?;

        let mut buf = Vec::new();
        f.read_to_end(&mut buf)
            .map_err(WadError::CouldntReadHeader)?;
        let mut de = wad_de::WadDeserializer::from_bytes(&buf);
        let header = WadHeader::deserialize(&mut de)?;
        let mut directory = Vec::with_capacity(header.num_lumps as usize);
        f.seek(SeekFrom::Start(header.directory_offset as u64))
            .map_err(WadError::CouldntReadHeader)?;
        for _ in 0..header.num_lumps {
            directory.push(DirectoryEntry::deserialize(&mut de)?);
        }

        Ok(Wad {
            header,
            directory: Directory(directory),
        })
    }

    /// Returns the number of lumps in the WAD.
    fn num_lumps(&self) -> usize {
        self.directory.0.len()
    }

    /// Returns the lump at the given index.
    fn lump(&self, index: usize) -> Result<Lump, WadError> {
        let mut f = File::open("doom.wad").map_err(WadError::CouldntReadHeader)?;
        Lump::new(&mut f, &self.directory.0[index])
    }
}
