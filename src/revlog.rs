use core::fmt::Write;
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
struct RevlogChunk {
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

struct RevlogEntry<'a> {
    /// Pointer to the data block
    chunk: &'a RevlogChunk,
    /// Byte offset of chunk in the index file
    byte_offset: i32,
    revlog: &'a Revlog,
}

impl<'a> RevlogEntry<'a> {
    fn advance(self) -> Result<Option<RevlogEntry<'a>>> {
        let next = (self.byte_offset + self.chunk.comp_len() + 64) as u64;
        if next == self.revlog.index.len {
            println!("Exactly reached the end.");
            return Ok(None);
        }
        println!("Advancing from entry at {} with comp_len of {} to {}.",
                 self.byte_offset,
                 self.chunk.comp_len(),
                 next);
        let result = try!(self.revlog.index_entry_at_byte(next as isize));
        Ok(Some(result))
    }
}

struct RevlogIterator<'a> {
    revlog: &'a Revlog,
    /// None if iter hasn't begun
    cur: Option<&'a RevlogEntry<'a>>,
}

impl<'a> Iterator for RevlogIterator<'a> {
    type Item = Result<RevlogEntry<'a>>;
    fn next(&mut self) -> Option<Self::Item> {
        None
    }
}

pub struct Revlog {
    index: MappedData,
    data: Option<MappedData>,
    inline: bool,
    generaldelta: bool,
    offset_table: Vec<u64>,
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
            None
        } else {
            let mut y = String::from(&path[..path.len() - 2]);
            y.push_str(".d");
            println!("opening data: {:?}", y);
            Some(try!(util::MappedData::open(&*y)))
        };

        let mut result = Revlog {
            index: index,
            data: data,
            inline: inline,
            generaldelta: generaldelta,
            offset_table: vec![0],
        };
        result.init();
        return Ok(result);
    }

    fn init(&mut self) {
        assert!(self.inline != self.data.is_some());
        // if self.inline {
        // self.scan_index();
        // }
        //
    }

    // fn scan_index(&mut self) {
    // loop {
    // let mut entry = self.entry(0).unwrap();
    // println!("{}", entry);
    // }
    // }
    //

    /// An index entry is 64 bytes long.
    /// If the revision data is not inline, then the index entries
    /// must be aligned at 64-byte boundaries. Otherwise, they may
    /// be anywhere.
    fn index_entry_at_byte(&self, i: isize) -> Result<RevlogEntry> {
        if !self.inline {
            expect!(i % 64 == 0);
        }

        let chunk: &RevlogChunk = self.index.extract_value(i);
        let result = RevlogEntry {
            chunk: chunk,
            byte_offset: i as i32,
            revlog: &self,
        };
        expect!(result.chunk.c_node_id[20..] == [0; 12]);
        return Ok(result);
    }

    /// Fetches revision i.
    /// The special revision -1 always exists.
    /// If the revision data is inline, then the first access incurs
    /// a full scan of the file.
    fn entry(&self, i: isize) -> Result<RevlogEntry> {
        if self.inline {
            // let (i, offset) = (self.offset_table.len() - 1,
            // self.offset_table.last().unwrap());
            // println!("Last offset: rev {} is at {}", i, offset);
            //
            println!("FIXME this is not smart yet");
            let mut entry = try!(self.index_entry_at_byte(0));
            let mut cur = 0;
            loop {
                if cur == i {
                    return Ok(entry);
                }
                entry = match try!(entry.advance()) {
                    Some(next_entry) => {
                        cur = cur + 1;
                        next_entry
                    }
                    None => {
                        let mut s = String::new();
                        write!(s, "No revision {}", i);
                        return Err(From::from(s));
                    }
                }
            }
        } else {
            self.index_entry_at_byte(i * 64)
        }
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
        write!(f,
               "comp_len: {}, uncomp_len: {}, base_rev: {}, link_rev: {}, parent_1: {}, \
                parent_2: {}, node_id: {}",
               self.chunk.comp_len(),
               self.chunk.uncomp_len(),
               self.chunk.base_rev(),
               self.chunk.link_rev(),
               self.chunk.parent_1(),
               self.chunk.parent_2(),
               self.chunk.c_node_id().to_hex())
    }
}
