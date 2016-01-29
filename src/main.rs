extern crate core;
extern crate mmap;
extern crate rustc_serialize;

mod revlog {

    use core::fmt::Write;
    use mmap::{MapOption, MemoryMap};
    use rustc_serialize::hex::ToHex;
    use std::os::unix::io::AsRawFd;
    use std::error;
    use std::fs;
    use std::fmt;
    use std::result;

    type Result<T> = result::Result<T, Box<error::Error>>;

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

    fn mmap_helper(path: &str) -> Result<MemoryMap> {
        let attr = fs::metadata(path).unwrap();
        assert!(attr.is_file(), "{} isn't a file", path);
        let f = try!(fs::File::open(path));
        let opts = &[MapOption::MapReadable, MapOption::MapFd(f.as_raw_fd())];
        match MemoryMap::new(attr.len() as usize, opts) {
            Ok(m) => Ok(m),
            Err(e) => Err(Box::new(e)),
        }
    }

    impl Revlog {
        fn open(path: &str) -> Result<Revlog> {
            assert!(path.ends_with(".i"));
            let index = try!(mmap_helper(path));

            // Read the flags from the first entry to store some
            // important globals

            let data;
            let data_path;
            let inline = true;
            if inline {
                data = None;
                data_path = None;
            } else {
                let mut y = String::from(&path[..path.len() - 2]);
                y.push_str(".d");
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

        fn entry(&self, i: isize) -> RevlogEntry {
            let chunk: &RevlogChunk = unsafe {
                let p = self.index.data().offset(i) as *const [u8; 64];
                dump_revlog_hex(&*p);
                &*(p as *const RevlogChunk)
            };
            let result = RevlogEntry {
                chunk: chunk,
                byte_offset: i,
            };
            debug_assert!(result.chunk.c_node_id[20..] == [0; 12],
                          "Misaligned chunk (missing id padding)");
            return result;
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

    pub fn read_revlog(path: &str) {
        let revlog = Revlog::open(path).unwrap();
        println!("{}:", revlog.index_path);
        let entry = revlog.entry(0);
        println!("{}", entry);
        let entry = revlog.entry(65);
        println!("{}", entry);
        let entry = revlog.entry(129);
        println!("{}", entry);
        println!("");
    }

}  // mod revlog

fn main() {
    for path in std::env::args().skip(1) {
        revlog::read_revlog(&path);
    }
}
