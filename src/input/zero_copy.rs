use std::alloc::{GlobalAlloc, Layout};
use std::ffi::c_void;
use std::io::{self, BufRead, Read};
use std::mem::forget;

use bincode::BincodeRead;
use libmimalloc_sys::{
    mi_free, mi_is_in_heap_region, mi_malloc_aligned, mi_realloc_aligned, mi_zalloc_aligned,
};
use memmap2::MmapMut;

struct TolerentAllocator;

unsafe impl GlobalAlloc for TolerentAllocator {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        mi_malloc_aligned(layout.size(), layout.align()).cast::<u8>()
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        mi_zalloc_aligned(layout.size(), layout.align()).cast::<u8>()
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = ptr.cast::<c_void>();
        if mi_is_in_heap_region(p) {
            mi_realloc_aligned(p, new_size, layout.align()).cast::<u8>()
        } else {
            GlobalAlloc::realloc(self, ptr, layout, new_size)
        }
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        let p = ptr.cast::<c_void>();
        if mi_is_in_heap_region(p) {
            mi_free(p);
        }
    }
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
