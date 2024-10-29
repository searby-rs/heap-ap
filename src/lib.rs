#![allow(unused)]
#![cfg_attr(feature = "allocator-api", feature(allocator_api))]
#![cfg_attr(feature = "allocator-api", feature(alloc_layout_extra))]
#![cfg_attr(feature = "allocator-api", feature(slice_ptr_get))]

extern crate cheap as _;
extern crate libc;
extern crate std;

use std::{
    alloc::GlobalAlloc,
    alloc::Layout,
    ptr::NonNull,
    hint,
    marker::PhantomPinned
};
#[cfg(feature = "allocator-api")]
use std::alloc::{Allocator, AllocError};

use libc::{size_t, c_void};

extern "C" {
    fn allocate(size: size_t, align: size_t) -> *mut c_void;
    fn allocate_zeroed(size: size_t, align: size_t) -> *mut c_void;
    fn reallocate(ptr: *mut c_void, size: size_t, align: size_t, new_size: size_t) -> *mut c_void;
    fn deallocate(ptr: *mut c_void, size: size_t, align: size_t);
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Heap {
    _pinned: PhantomPinned,
}

impl Heap {
    pub const fn new() -> Heap {
        Heap {
            _pinned: PhantomPinned,
        }
    }
}

unsafe impl Send for Heap {}
unsafe impl Sync for Heap {}

unsafe impl GlobalAlloc for Heap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size() as size_t;
        let align = layout.align() as size_t;
        allocate(size, align) as *mut u8
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let size = layout.size() as size_t;
        let align = layout.align() as size_t;
        deallocate(ptr as *mut c_void, size, align)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let size = layout.size() as size_t;
        let align = layout.align() as size_t;
        reallocate(ptr as *mut c_void, size, align, new_size as size_t) as *mut u8
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let size = layout.size() as size_t;
        let align = layout.align() as size_t;
        allocate_zeroed(size, align) as *mut u8
    }
}

#[cfg(feature = "allocator-api")]

impl Heap {
    #[inline]
    fn alloc_impl(&self, layout: Layout, zeroed: bool) -> Result<NonNull<[u8]>, AllocError> {
        match layout.size() {
            0 => Ok(NonNull::slice_from_raw_parts(layout.dangling(), 0)),
            size => unsafe {
                let raw = if zeroed {
                    self.alloc_zeroed(layout)
                } else {
                    self.alloc(layout)
                };
                let ptr = NonNull::new(raw).ok_or(AllocError)?;
                Ok(NonNull::slice_from_raw_parts(ptr, size))
            },
        }
    }
    #[inline]
    unsafe fn grow_impl(&self, ptr: NonNull<u8>, old_layout: Layout, new_layout: Layout, zeroed: bool) -> Result<NonNull<[u8]>, AllocError> {
        debug_assert!(new_layout.size() >= old_layout.size(), "new_layout.size() must be greater then or equal to old_layout.size()");
        match old_layout.size() {
            0 => self.alloc_impl(new_layout, zeroed),
            old_size if old_layout.align() == new_layout.align() => unsafe {
                let new_size = new_layout.size();
                hint::assert_unchecked(new_size >= old_layout.size());
                let raw = self.realloc(ptr.as_ptr(), old_layout, new_size);
                let ptr = NonNull::new(raw).ok_or(AllocError)?;
                if zeroed {
                    raw.add(old_size).write_bytes(0, new_size - old_size);
                }
                Ok(NonNull::slice_from_raw_parts(ptr, new_size))
            },
            old_size => unsafe {
                let new = self.alloc_impl(new_layout, zeroed)?;
                std::ptr::copy_nonoverlapping(ptr.as_ptr(), new.as_mut_ptr(), old_size);
                self.deallocate(ptr, old_layout);
                Ok(new)
            },
        }
    }
}

#[cfg(feature = "allocator-api")]
unsafe impl Allocator for Heap {
    #[inline]
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        self.alloc_impl(layout, false)
    }
    #[inline]
    fn allocate_zeroed(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        self.alloc_impl(layout, true)
    }
    #[inline]
    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        if layout.size() != 0 {
            unsafe {
                self.dealloc(ptr.as_ptr(), layout)
            }
        }
    }
    #[inline]
    unsafe fn grow(
            &self,
            ptr: NonNull<u8>,
            old_layout: Layout,
            new_layout: Layout,
        ) -> Result<NonNull<[u8]>, AllocError> {
        unsafe {
            self.grow_impl(ptr, old_layout, new_layout, false)
        }
    }
    #[inline]
    unsafe fn grow_zeroed(
            &self,
            ptr: NonNull<u8>,
            old_layout: Layout,
            new_layout: Layout,
        ) -> Result<NonNull<[u8]>, AllocError> {
        unsafe {
            self.grow_impl(ptr, old_layout, new_layout, true)
        }
    }
    #[inline]
    unsafe fn shrink(
            &self,
            ptr: NonNull<u8>,
            old_layout: Layout,
            new_layout: Layout,
        ) -> Result<NonNull<[u8]>, AllocError> {
        debug_assert!(new_layout.size() <= old_layout.size(), "new_layout.size() must be smaller then or equal to old_layout.size()");

        match new_layout.size() {
            0 => unsafe {
                self.deallocate(ptr, old_layout);
                Ok(NonNull::slice_from_raw_parts(new_layout.dangling(), 0))
            },
            new_size if old_layout.align() == new_layout.align() => unsafe {
                hint::assert_unchecked(new_size <= old_layout.size());
                let raw = self.realloc(ptr.as_ptr(), old_layout, new_size);
                let ptr = NonNull::new(raw).ok_or(AllocError)?;
                Ok(NonNull::slice_from_raw_parts(ptr, new_size)) 
            },
            new_size => unsafe {
                let new = self.allocate(new_layout)?;
                std::ptr::copy_nonoverlapping(ptr.as_ptr(), new.as_mut_ptr(), new_size);
                self.deallocate(ptr, old_layout);
                Ok(new)
            },
        }
    }
}

#[cfg(feature = "allocator-api")]
pub type Vec<T> = std::vec::Vec<T, Heap>;
#[cfg(feature = "allocator-api")]
pub type Box<T> = std::boxed::Box<T, Heap>;


