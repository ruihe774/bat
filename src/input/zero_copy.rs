#[allow(unused_imports)]
use zwrite::{write, writeln};

use std::alloc::{GlobalAlloc, Layout};
use std::ffi::c_void;
use std::io::{self, BufRead, Read};
use std::mem::forget;
#[cfg(debug_assertions)]
use std::sync::atomic::{AtomicBool, Ordering};

use bincode::BincodeRead;
#[allow(clippy::wildcard_imports)] // too many imports
use libmimalloc_sys::*;
use memmap2::MmapMut;

#[cfg(debug_assertions)]
use crate::error::Error;

struct TolerentAllocator;

#[cfg(debug_assertions)]
pub(crate) static MEMBRANE: AtomicBool = AtomicBool::new(false);

unsafe impl GlobalAlloc for TolerentAllocator {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        #[cfg(debug_assertions)]
        if MEMBRANE.load(Ordering::Relaxed) {
            break_membrane();
        }
        mi_malloc_aligned(layout.size(), layout.align()).cast::<u8>()
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        #[cfg(debug_assertions)]
        if MEMBRANE.load(Ordering::Relaxed) {
            break_membrane();
        }
        mi_zalloc_aligned(layout.size(), layout.align()).cast::<u8>()
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        #[cfg(debug_assertions)]
        if MEMBRANE.load(Ordering::Relaxed) {
            break_membrane();
        }
        let p = ptr.cast::<c_void>();
        if mi_is_in_heap_region(p) {
            mi_realloc_aligned(p, new_size, layout.align()).cast::<u8>()
        } else {
            GlobalAlloc::realloc(self, ptr, layout, new_size)
        }
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let p = ptr.cast::<c_void>();
        if mi_is_in_heap_region(p) {
            mi_free_size_aligned(p, layout.size(), layout.align());
        }
    }
}

#[cfg(debug_assertions)]
#[cold]
fn break_membrane() {
    MEMBRANE.store(false, Ordering::Relaxed);
    eprintln!("{:?}", Error::msg("membrane broken"));
    MEMBRANE.store(true, Ordering::Relaxed);
}

#[global_allocator]
static GLOBAL: TolerentAllocator = TolerentAllocator;

pub(crate) fn leak_mmap(mut mmap: MmapMut) -> &'static mut [u8] {
    let ptr = mmap.as_mut_ptr();
    let len = mmap.len();
    let slice = unsafe { std::slice::from_raw_parts_mut(ptr, len) };
    forget(mmap);
    slice
}

pub(crate) struct LeakySliceReader {
    ptr: *mut u8,
    len: usize,
}

impl LeakySliceReader {
    pub fn new(slice: &'static mut [u8]) -> LeakySliceReader {
        let ptr = slice.as_mut_ptr();
        let len = slice.len();
        assert!(
            unsafe { !mi_is_in_heap_region(ptr.cast::<c_void>()) },
            "slice not leaky"
        );
        LeakySliceReader { ptr, len }
    }

    pub fn from_leaky_vec(mut vec: Vec<u8>) -> LeakySliceReader {
        Self::new(unsafe { std::slice::from_raw_parts_mut(vec.as_mut_ptr(), vec.len()) })
    }
}

impl Read for LeakySliceReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let slice = self.fill_buf()?;
        let len = slice.len().min(buf.len());
        buf[..len].copy_from_slice(&slice[..len]);
        self.consume(len);
        Ok(len)
    }
}

impl BufRead for LeakySliceReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        Ok(unsafe { std::slice::from_raw_parts(self.ptr, self.len) })
    }

    fn consume(&mut self, amt: usize) {
        assert!(amt <= self.len, "comsume amount larger than length");
        self.ptr = unsafe { self.ptr.add(amt) };
        self.len -= amt;
    }
}

impl LeakySliceReader {
    fn get_byte_slice(&mut self, length: usize) -> bincode::Result<&'static mut [u8]> {
        if self.len < length {
            return Err(Box::new(bincode::ErrorKind::Io(io::Error::from(
                io::ErrorKind::UnexpectedEof,
            ))));
        }
        let slice = unsafe { std::slice::from_raw_parts_mut(self.ptr, length) };
        self.consume(length);
        Ok(slice)
    }
}

impl<'storage> BincodeRead<'storage> for LeakySliceReader {
    fn get_byte_buffer(&mut self, length: usize) -> bincode::Result<Vec<u8>> {
        let slice = self.get_byte_slice(length)?;
        let vec = unsafe { Vec::from_raw_parts(slice.as_mut_ptr(), length, length) };
        Ok(vec)
    }

    fn forward_read_bytes<V>(&mut self, length: usize, visitor: V) -> bincode::Result<V::Value>
    where
        V: serde::de::Visitor<'storage>,
    {
        visitor.visit_borrowed_bytes(self.get_byte_slice(length)?)
    }

    fn forward_read_str<V>(&mut self, length: usize, visitor: V) -> bincode::Result<V::Value>
    where
        V: serde::de::Visitor<'storage>,
    {
        visitor.visit_borrowed_str(
            std::str::from_utf8(self.get_byte_slice(length)?)
                .map_err(|e| Box::new(bincode::ErrorKind::InvalidUtf8Encoding(e)))?,
        )
    }
}
