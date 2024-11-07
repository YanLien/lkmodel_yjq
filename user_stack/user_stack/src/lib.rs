//! User-space stack management implementation
//!
//! This module provides functionality for managing user-space stacks, including:
//! - Stack allocation and initialization
//! - Safe data pushing operations
//! - String handling on the stack
//! - Proper alignment management
//!
//! # Features
//! - No standard library dependency
//! - Automatic alignment handling
//! - Safe memory operations
//! - Support for various data types
//!

#![no_std]

use core::{mem::align_of, mem::size_of_val};

/// Represents a user-space stack with automatic alignment management
pub struct UserStack {
    _base: usize,
    sp: usize,
    ptr: usize,
}

impl UserStack {
    /// Creates a new user stack instance
    pub fn new(base: usize, ptr: usize) -> Self {
        Self {
            _base: base,
            sp: base,
            ptr,
        }
    }

    /// Returns the current stack pointer position
    pub fn get_sp(&self) -> usize {
        self.sp
    }

    /// Pushes an array of data onto the stack
    pub fn push<T: Copy>(&mut self, data: &[T]) {
        let origin = self.sp;
        self.sp -= size_of_val(data);
        self.sp -= self.sp % align_of::<T>();
        self.ptr -= origin - self.sp;
        unsafe {
            core::slice::from_raw_parts_mut(self.ptr as *mut T, data.len())
                .copy_from_slice(data);
        }
    }

    /// Pushes a string onto the stack, adding a null terminator
    pub fn push_str(&mut self, str: &str) -> usize {
        self.push(&[b'\0']);
        self.push(str.as_bytes());
        self.sp
    }
}

/// Initializes the user stack subsystem
pub fn init() {
    axconfig::init_once!();
    axalloc::init();
}
