extern crate core;
extern crate mmap;
extern crate rustc_serialize;

mod revlog {

    use core::fmt::Write;
    use mmap::{MapOption, MemoryMap};
    use rustc_serialize::hex::ToHex;
    use std::mem;
    use std::os::unix::io::AsRawFd;
    use std::error;
    use std::fs;
    use std::fmt;
    use std::result;

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
            expect!($e, "{}", stringify!($e));
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
    #[derive(Debug)]
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

    #[derive(Debug)]
    struct RevlogEntry<'a> {
        /// Pointer to the data block
        chunk: &'a RevlogChunk,
        /// Byte offset of chunk in the index file
        byte_offset: isize,
    }

    struct Revlog {
        index: MemoryMap,
        index_path: String,
        data: Option<MemoryMap>,
        data_path: Option<String>,
        inline: bool,
    }

    fn extract_value<T>(data: &MemoryMap, index: isize) -> &T {
        assert!(index >= 0);
        assert!(index as usize + mem::size_of::<T>() < data.len());
        unsafe {
            let p: *const T = data.data().offset(index) as *const T;
            &*p
        }
    }

    fn mmap_helper(path: &str) -> Result<MemoryMap> {
        let attr = try!(fs::metadata(path));
        expect!(attr.is_file());
        let f = try!(fs::File::open(path));
        let opts = &[MapOption::MapReadable, MapOption::MapFd(f.as_raw_fd())];
        MemoryMap::new(attr.len() as usize, opts).map_err(From::from)
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
                let first_chunk: &RevlogChunk = extract_value(&index, 0);
                let offset_flags = u64::from_be(first_chunk.offset_flags);
                (offset_flags >> 32) as u32
            };
            println!("flags: {:08x}", flags);
            expect!(flags & REVLOGNG != 0);
            let inline = (flags & REVLOGNGINLINEDATA) != 0;
            let generaldelta = (flags & REVLOGGENERALDELTA) != 0;
            println!("inline: {}", inline);
            println!("generaldelta: {}", generaldelta);

            let data;
            let data_path;
            if inline {
                data = None;
                data_path = None;
            } else {
                let mut y = String::from(&path[..path.len() - 2]);
                y.push_str(".d");
                println!("opening data: {:?}", y);
                data = Some(try!(mmap_helper(&*y)));
                data_path = Some(y);
            }

            let mut result = Revlog {
                index_path: String::from(path),
                index: index,
                inline: inline,
                data_path: data_path,
                data: data,
            };
            result.init();
            return Ok(result);
        }

        fn init(&mut self) {}

        /// An index entry is 64 bytes long.
        /// If the revision data is not inline, then the index entries
        /// must be aligned at 64-byte boundaries. Otherwise, they may
        /// be anywhere.
        fn index_entry_at_byte(&self, i: isize) -> Result<RevlogEntry> {
            if self.inline {
                expect!(i % 64 == 0);
            }

            let chunk: &RevlogChunk = extract_value(&self.index, i);
            let result = RevlogEntry {
                chunk: chunk,
                byte_offset: i,
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
                Err(From::from("unimplemented"))
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
                   i32::from_be(self.chunk.comp_len),
                   i32::from_be(self.chunk.uncomp_len),
                   i32::from_be(self.chunk.base_rev),
                   i32::from_be(self.chunk.link_rev),
                   i32::from_be(self.chunk.parent_1),
                   i32::from_be(self.chunk.parent_2),
                   self.chunk.c_node_id[..20].to_hex())
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
        /*
        println!("{}", revlog.entry(0).unwrap());
        println!("{}", revlog.entry(1).unwrap());
        println!("{}", revlog.entry(2).unwrap());
        */
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
