extern crate core;
extern crate mmap;
extern crate rustc_serialize;

// TODO: Either generalize this code to Seek+Read, or extend MemoryMap with
// those traits.

mod revlog {

    use core::fmt::Write;
    use mmap::{MapOption, MemoryMap};
    use rustc_serialize::hex::ToHex;
    use std::os::unix::io::AsRawFd;
    use std::{mem, error, fs, fmt, result};

    type Result<T> = result::Result<T, Box<error::Error>>;

    /// Like assert!, but returns a Result(Err) instead of panicking.
    macro_rules! expect {
        ( $e:expr, $($t:tt)+ ) => {
            if !$e {
                use std::fmt::Write;
                let mut s = String::new();
                write!(s, $($t)+).unwrap();
                return Err(From::from(s))
            }
        };
        ( $e:expr ) => {
            expect!($e, "expect failed: {:?}", stringify!($e));
        };
    }

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
                return Ok(None)
            }
            println!("Advancing. I start at {}, comp_len is {}, \
            and headers are 64, so the next entry starts at {}. \
            Giving it a shot...",
                     self.byte_offset,
                     self.chunk.comp_len(),
                     next);
            let result = try!(self.revlog.index_entry_at_byte(next as isize));
            Ok(Some(result))
        }
    }

    // Note that MemoryMap::len() is rounded up to 4096 blocks.
    struct MappedData {
        mmap: MemoryMap,
        path: String,
        len: u64,
    }

    struct Revlog {
        index: MappedData,
        data: Option<MappedData>,
        inline: bool,
        generaldelta: bool,
        offset_table: Vec<u64>,
    }

    impl MappedData {
        // In general, it's not safe to treat a shared mmap as a slice.
        // Safety in this case relies on the contents of the file never
        // being overwritten, which Mercurial promises.
        // Concurrent truncation will cause SIGBUS.
        //
        // Safety also of course relies on the bounds checking we do here,
        // and on bounding all borrows to the lifetime of the mmap itself,
        // which is given by the signature of this function; in principle
        // this means that the rest of the module can be safe code.
        fn extract_value<T>(&self, index: isize) -> &T {
            assert!(index >= 0);
            assert!(index as usize + mem::size_of::<T>() < self.len as usize,
                    "{} + {} > {}",
                    index,
                    mem::size_of::<T>(),
                    self.len as usize);
            unsafe {
                let p: *const T = self.mmap.data().offset(index) as *const T;
                &*p
            }
        }
    }

    fn mmap_helper(path: &str) -> Result<MappedData> {
        let attr = try!(fs::metadata(path));
        expect!(attr.is_file());
        let f = try!(fs::File::open(path));
        let opts = &[MapOption::MapReadable, MapOption::MapFd(f.as_raw_fd())];
        let m = try!(MemoryMap::new(attr.len() as usize, opts));
        let result = MappedData {
            mmap: m,
            path: String::from(path),
            len: attr.len(),
        };
        Ok(result)
    }

    impl Revlog {
        fn open(path: &str) -> Result<Revlog> {
            expect!(path.ends_with(".i"));
            println!("");
            println!("opening index: {:?}", path);
            let index = try!(mmap_helper(path));

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
                Some(try!(mmap_helper(&*y)))
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
            /*
            if self.inline {
                self.scan_index();
            }
            */
        }

        /*
        fn scan_index(&mut self) {
            loop {
                let mut entry = self.entry(0).unwrap();
                println!("{}", entry);
            }
        }
        */

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
                        },
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

    fn dump_revlog_hex(data: &[u8]) {
        if data.len() == 0 {
            return;
        }
        let (x, xs) = data.split_at(16);
        println!("{}", x.to_hex());
        dump_revlog_hex(xs);
    }

    pub fn read_revlog(path: &str) -> result::Result<(), Box<error::Error>> {
        let revlog = try!(Revlog::open(path));
        for i in 0..3 {
            println!("{} => {}", i, try!(revlog.entry(i)));
        }
        Ok(())
    }

}  // mod revlog

fn main() {
    for path in std::env::args().skip(1) {
        match revlog::read_revlog(&path) {
            Ok(()) => (),
            Err(e) => println!("{}", e),
        }
    }
}
