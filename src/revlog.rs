use rustc_serialize::hex::ToHex;
use std::fmt;

use util;

use util::MappedData;
pub use util::Result;

const REVLOGV0: u32 = 0;
const REVLOGNG: u32 = 1;
const REVLOGNGINLINEDATA: u32 = (1 << 16);
const REVLOGGENERALDELTA: u32 = (1 << 17);

/// A low-level cursor into RevlogNG index entry.
/// For instance, these fields do not yet take into account:
/// - Conversion from big endian
/// - Masking the version out of the first offset_flags
/// - Selecting the first 20 bytes of c_node_id
#[repr(C)]
pub struct RevlogChunk {
    offset_flags: u64,
    comp_len: i32,
    uncomp_len: i32,
    base_rev: i32,
    link_rev: i32,
    parent_1: i32,
    parent_2: i32,
    c_node_id: [u8; 32],
}

/// Accessors that decode the data somewhat.
impl RevlogChunk {
    fn offset_flags(&self) -> u64 {
        u64::from_be(self.offset_flags)
    }
    fn offset(&self) -> u64 {
        u64::from_be(self.offset_flags) >> 16
    }
    fn comp_len(&self) -> i32 {
        i32::from_be(self.comp_len)
    }
    fn uncomp_len(&self) -> i32 {
        i32::from_be(self.uncomp_len)
    }
    fn base_rev(&self) -> i32 {
        i32::from_be(self.base_rev)
    }
    fn link_rev(&self) -> i32 {
        i32::from_be(self.link_rev)
    }
    fn parent_1(&self) -> i32 {
        i32::from_be(self.parent_1)
    }
    fn parent_2(&self) -> i32 {
        i32::from_be(self.parent_2)
    }
    fn c_node_id(&self) -> &[u8] {
        &self.c_node_id[..20]
    }
}

#[derive(Clone)]
pub struct RevlogEntry<'a> {
    pub revlog: &'a Revlog,
    /// Pointer to the index block
    pub chunk: &'a RevlogChunk,
    /// Byte offset of chunk in the index file
    pub byte_offset: i32,
    /// Data frame can either be a slice of the inline data or
    /// a slice of the external data.
    pub data: &'a [u8],
}

impl<'a> RevlogEntry<'a> {
    // Precondition: inline
    fn inline_advance(self) -> Result<Option<RevlogEntry<'a>>> {
        let next = (self.byte_offset + self.chunk.comp_len() + 64) as u64;
        if next == self.revlog.index.len {
            return Ok(None);
        }
        let result = try!(self.revlog.index_entry_at_byte(next as isize));
        Ok(Some(result))
    }
}

pub struct RevlogIterator<'a> {
    revlog: &'a Revlog,
    /// None if iter hasn't begun
    cur: Option<RevlogEntry<'a>>,
}

impl<'a> Iterator for RevlogIterator<'a> {
    type Item = Result<RevlogEntry<'a>>;
    fn next(&mut self) -> Option<Self::Item> {
        let next = match self.cur {
            None => {
                match self.revlog.index_entry_at_byte(0) {
                    Ok(entry) => Some(entry),
                    Err(e) => return Some(Err(e)),
                }
            }
            Some(ref prev) => {
                if self.revlog.inline() {
                    match prev.clone().inline_advance() {
                        Ok(None) => None,
                        Ok(Some(entry)) => Some(entry),
                        Err(e) => return Some(Err(e)),
                    }
                } else {
                    let next_offset = (prev.byte_offset + 64) as u64;
                    if next_offset == self.revlog.index.len {
                        None
                    } else {
                        match self.revlog.index_entry_at_byte(next_offset as isize) {
                            Ok(entry) => Some(entry),
                            Err(e) => return Some(Err(e)),
                        }
                    }

                }
            }
        };
        self.cur = next.clone();
        return next.map(Ok);
    }
}

pub struct Revlog {
    index: MappedData,
    data: RevlogData,
    generaldelta: bool,
    offset_table: Vec<u64>,
}

/// Revlog data may either be inline in the index, or in a separate file.
/// (Inline should only be found in small files, as it requires a linear scan.)
enum RevlogData {
    Inline,
    Separate(MappedData),
}

impl Revlog {
    pub fn open(path: &str) -> Result<Revlog> {
        expect!(path.ends_with(".i"));
        println!("");
        println!("opening index: {:?}", path);
        let index = try!(util::MappedData::open(path));

        // Read the flags from the first entry to store some
        // important globals
        let flags: u32 = {
            let first_chunk: &RevlogChunk = index.extract_value(0);
            (first_chunk.offset_flags() >> 32) as u32
        };
        println!("flags: {:08x}", flags);
        expect!(flags & REVLOGNG != 0);
        let inline = (flags & REVLOGNGINLINEDATA) != 0;
        let generaldelta = (flags & REVLOGGENERALDELTA) != 0;
        println!("inline: {}", inline);
        println!("generaldelta: {}", generaldelta);

        let data = if inline {
            RevlogData::Inline
        } else {
            let mut y = String::from(&path[..path.len() - 2]);
            y.push_str(".d");
            println!("opening data: {:?}", y);
            RevlogData::Separate(try!(util::MappedData::open(&*y)))
        };

        let result = Revlog {
            index: index,
            data: data,
            generaldelta: generaldelta,
            offset_table: vec![0],
        };
        return Ok(result);
    }

    fn inline(&self) -> bool {
        match self.data {
            RevlogData::Inline => true,
            _ => false,
        }
    }

    /// An index entry is 64 bytes long.
    /// If the revision data is not inline, then the index entries
    /// must be aligned at 64-byte boundaries. Otherwise, they may
    /// be anywhere.
    fn index_entry_at_byte(&self, i: isize) -> Result<RevlogEntry> {
        if !self.inline() {
            expect!(i % 64 == 0);
        }

        let chunk: &RevlogChunk = self.index.extract_value(i);
        let data = match self.data {
            RevlogData::Inline => self.index.extract_slice(i + 64, chunk.comp_len() as usize),
            RevlogData::Separate(ref data) => {
                let offset = if i == 0 { 0 } else { chunk.offset() as isize };
                data.extract_slice(offset, chunk.comp_len() as usize)
            }
        };
        let result = RevlogEntry {
            revlog: &self,
            chunk: chunk,
            byte_offset: i as i32,
            data: data,
        };
        expect!(result.chunk.c_node_id[20..] == [0; 12]);
        return Ok(result);
    }

    pub fn iter(&self) -> RevlogIterator {
        RevlogIterator {
            revlog: self,
            cur: None,
        }
    }
}

impl<'a> fmt::Display for RevlogEntry<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "chunk<{}>", self.chunk)
    }
}

impl fmt::Display for RevlogChunk {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f,
               "offset: {}, comp_len: {}, uncomp_len: {}, base_rev: {}, link_rev: {}, parent_1: \
                {}, parent_2: {}, node_id: {}",
               self.offset(),
               self.comp_len(),
               self.uncomp_len(),
               self.base_rev(),
               self.link_rev(),
               self.parent_1(),
               self.parent_2(),
               self.c_node_id().to_hex())
    }
}
