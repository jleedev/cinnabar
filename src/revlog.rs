use util;
use util::MappedData;
pub use util::Result;

const REVLOGV0: u32 = 0;
const REVLOGNG: u32 = 1;
const REVLOGNGINLINEDATA: u32 = (1 << 16);
const REVLOGGENERALDELTA: u32 = (1 << 17);

const NULL_ID: &'static [u8] = &[0u8; 20];

/// A low-level cursor into RevlogNG index entry.
///
/// For instance, these fields do not yet take into account:
/// - Conversion from big endian
/// - Masking the version out of the first offset_flags
/// - Distinguishing between offset and flags for the first rev
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
    pub fn comp_len(&self) -> i32 {
        i32::from_be(self.comp_len)
    }
    pub fn uncomp_len(&self) -> i32 {
        i32::from_be(self.uncomp_len)
    }
    pub fn base_rev(&self) -> i32 {
        i32::from_be(self.base_rev)
    }
    pub fn link_rev(&self) -> i32 {
        i32::from_be(self.link_rev)
    }
    pub fn parent_1(&self) -> i32 {
        i32::from_be(self.parent_1)
    }
    pub fn parent_2(&self) -> i32 {
        i32::from_be(self.parent_2)
    }
    pub fn c_node_id(&self) -> &[u8] {
        &self.c_node_id[..20]
    }
}

#[derive(Clone)]
pub struct RevlogEntry<'a> {
    pub revlog: &'a Revlog,
    /// This rev's position in the index
    pub revno: i32,
    /// Pointer to the index block
    pub chunk: &'a RevlogChunk,
    /// Byte offset of chunk in the index file
    pub byte_offset: isize,
    /// Data frame can either be a slice of the inline data or
    /// a slice of the external data.
    pub data: &'a [u8],
}

impl<'a> RevlogEntry<'a> {
    // Precondition: inline
    fn inline_advance(self) -> Result<Option<RevlogEntry<'a>>> {
        let next = (self.byte_offset + self.chunk.comp_len() as isize + 64) as isize;
        if next == self.revlog.index.len {
            return Ok(None);
        }
        let result = try!(self.revlog.index_entry_at_byte(next as isize, None));
        Ok(Some(result))
    }

    // Look up the node ids of the parents from the revs
    pub fn parent_1_id(&self) -> Result<&[u8]> {
        let p1 = self.chunk.parent_1();
        if p1 == -1 {
            return Ok(NULL_ID);
        }
        let entry = try!(self.revlog.index(p1));
        Ok(entry.chunk.c_node_id())
    }

    pub fn parent_2_id(&self) -> Result<&[u8]> {
        let p2 = self.chunk.parent_2();
        if p2 == -1 {
            return Ok(NULL_ID);
        }
        let entry = try!(self.revlog.index(p2));
        Ok(entry.chunk.c_node_id())
    }

    pub fn offset(&self) -> u64 {
        if self.byte_offset == 0 {
            0
        } else {
            self.chunk.offset()
        }
    }

    pub fn base_rev(&self) -> i32 {
        let base = self.chunk.base_rev();
        if base == self.revno && self.revlog.generaldelta {
            -1
        } else {
            base
        }
    }

    pub fn delta_chain(&self) -> DeltaChain {
        DeltaChain { cur: Some(self.clone()) }
    }
}

/// An iterator over the raw bits of a delta chain
/// beginning with the specified rev and ending with the base
pub struct DeltaChain<'a> {
    // None if iteration is finished
    cur: Option<RevlogEntry<'a>>,
}

impl<'a> Iterator for DeltaChain<'a> {
    type Item = Result<&'a [u8]>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.cur.is_none() {
            return None;
        }
        let cur = self.cur.take().unwrap();
        let result = cur.data;
        let next_rev = cur.base_rev();
        self.cur = if next_rev == -1 || next_rev == cur.revno {
            None
        } else {
            Some(cur.revlog.index(next_rev).unwrap())
        };
        return Some(Ok(result));
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
                match self.revlog.index_entry_at_byte(0, None) {
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
                    let next_offset = prev.byte_offset + 64;
                    if next_offset == self.revlog.index.len {
                        None
                    } else {
                        match self.revlog.index_entry_at_byte(next_offset as isize, None) {
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
    /// Mmap of the index file.
    index: MappedData,
    /// Revlog data may either be inline in the index, or in a separate
    /// file. Inline should only be found in small files, as it requires
    /// a linear scan.)
    data: Option<MappedData>,
    /// Important flags extracted from the first rev.
    pub generaldelta: bool,
    /// If inline, a jump table is built.
    /// Mapping from rev no to byte_offset in the index.
    offset_table: Vec<isize>,
    /// Has init finished being called?
    _incomplete: bool,
}

impl Revlog {
    pub fn open(path: &str) -> Result<Revlog> {
        expect!(path.ends_with(".i"));
        println!("=====");
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
            generaldelta: generaldelta,
            offset_table: vec![],
            _incomplete: true,
        };
        try!(result.init());
        return Ok(result);
    }

    fn init(&mut self) -> Result<()> {
        assert!(self._incomplete);
        if !self.inline() {
            self._incomplete = false;
            return Ok(());
        }
        let mut offset_tmp = vec![];
        for (i, entry) in self.iter().enumerate() {
            let entry = try!(entry);
            offset_tmp.push(entry.byte_offset);
        }
        self.offset_table = offset_tmp;
        self._incomplete = false;
        Ok(())
    }

    fn inline(&self) -> bool {
        self.data.is_none()
    }

    fn revno_from_offset(&self, offset: isize) -> Result<i32> {
        if self._incomplete {
            // This is just supplementary data so the entry can know its
            // own revno. Omit it during construction.
            return Ok(-1);
        }
        if self.inline() {
            match self.offset_table.binary_search(&offset) {
                Ok(i) => Ok(i as i32),
                Err(i) => {
                    use std::fmt::Write;
                    let mut s = String::new();
                    write!(s, "Error finding revno for offset {}", offset).unwrap();
                    return Err(From::from(s));
                }
            }
        } else {
            Ok((offset / 64) as i32)
        }
    }

    /// An index entry is 64 bytes long.
    /// If the revision data is not inline, then the index entries
    /// must be aligned at 64-byte boundaries. Otherwise, they may
    /// be anywhere.
    fn index_entry_at_byte(&self, offset: isize, revno: Option<i32>) -> Result<RevlogEntry> {
        if !self.inline() {
            expect!(offset % 64 == 0);
        }

        let chunk: &RevlogChunk = self.index.extract_value(offset);
        let data = match self.data {
            None => self.index.extract_slice(offset + 64, chunk.comp_len() as usize),
            Some(ref data) => {
                let offset = if offset == 0 {
                    0
                } else {
                    chunk.offset() as isize
                };
                data.extract_slice(offset, chunk.comp_len() as usize)
            }
        };

        let revno = match revno {
            Some(x) => x,
            None => try!(self.revno_from_offset(offset)),
        };

        let result = RevlogEntry {
            revlog: &self,
            revno: revno,
            chunk: chunk,
            byte_offset: offset,
            data: data,
        };

        // Some quick sanity checks which are always true and can help
        // verify correctness:
        // - The 32 byte id field is 20 bytes of sha and 12 zero bytes
        // - The data frame when nonempty begins with
        //   null -> as is, including the null
        //   u -> as is, not including the u
        //   x -> gzip header
        // - All ids are positive signed integers
        expect!(result.chunk.c_node_id[20..] == [0; 12]);
        if data.len() > 0 {
            match data[0] as char {
                '\0' => (),
                'u' => (),
                'x' => (),
                c => expect!(false, "Weird data type {:?}", c),
            }
        }
        return Ok(result);
    }

    pub fn iter(&self) -> RevlogIterator {
        RevlogIterator {
            revlog: self,
            cur: None,
        }
    }

    pub fn len(&self) -> isize {
        if self.inline() {
            // We have a handy lookup table
            self.offset_table.len() as isize
        } else {
            // The index file is 64 bytes * the number of revs
            self.index.len / 64
        }
    }

    pub fn index(&self, index: i32) -> Result<RevlogEntry> {
        if self.inline() {
            expect!(index >= 0, "index {} is out of bounds", index);
            expect!(index < self.offset_table.len() as i32,
                    "index {} is bigger than {}",
                    index,
                    self.offset_table.len());
            let offset = self.offset_table[index as usize];
            return self.index_entry_at_byte(offset, Some(index));
        } else {
            return self.index_entry_at_byte(64 * index as isize, Some(index));
        }
    }
}
