use std::{mem, fs, result, error, slice};
use std::os::unix::io::AsRawFd;
use mmap;

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

pub type Result<T> = result::Result<T, Box<error::Error>>;

// Note that MemoryMap::len() is rounded up to 4096 blocks.
pub struct MappedData {
    pub mmap: mmap::MemoryMap,
    pub path: String,
    pub len: isize,
}

impl MappedData {
    pub fn open(path: &str) -> Result<MappedData> {
        let attr = try!(fs::metadata(path));
        expect!(attr.is_file());
        let f = try!(fs::File::open(path));
        let opts = &[mmap::MapOption::MapReadable, mmap::MapOption::MapFd(f.as_raw_fd())];
        let m = try!(mmap::MemoryMap::new(attr.len() as usize, opts));
        let result = MappedData {
            mmap: m,
            path: String::from(path),
            len: attr.len() as isize,
        };
        Ok(result)
    }

    // In general, it's not safe to treat a shared mmap as a slice.
    // Safety in this case relies on the contents of the file never
    // being overwritten, which Mercurial promises.
    // Concurrent truncation will cause SIGBUS.
    //
    // Safety also of course relies on the bounds checking we do here,
    // and on bounding all borrows to the lifetime of the mmap itself,
    // which is given by the signature of this function; in principle
    // this means that the rest of the module can be safe code.

    /// Borrow a value from the mmap, with bounds checking
    pub fn extract_value<T>(&self, index: isize) -> &T {
        assert!(index >= 0);
        assert!(index as usize + mem::size_of::<T>() <= self.len as usize,
                "{} + {} <= {}",
                index,
                mem::size_of::<T>(),
                self.len as usize);
        unsafe {
            let p = self.mmap.data().offset(index) as *const T;
            &*p
        }
    }

    /// Borrow a byte slice from the mmap, with bounds checking
    pub fn extract_slice(&self, index: isize, len: usize) -> &[u8] {
        assert!(index >= 0);
        assert!(index + len as isize <= self.len as isize,
                "{} + {} <= {}",
                index,
                len as isize,
                self.len as isize);
        unsafe { slice::from_raw_parts(self.mmap.data().offset(index), len) }
    }
}
