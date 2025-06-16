use std::cmp::min;
use std::io::{Result, Write};
use std::ops::{Deref, DerefMut};

const PAGE_SIZE: usize = 1 << 30; // 1 GB per page

pub struct BigBuffer {
    pub byte_length: usize,
    pub buffers: Vec<Vec<u8>>, // Each Vec<u8> is a page
}

impl BigBuffer {
    pub fn new(size: usize) -> Self {
        let mut buffers = Vec::new();
        let mut remaining = size;

        while remaining > 0 {
            let page_len = min(remaining, PAGE_SIZE);
            buffers.push(vec![0u8; page_len]);
            remaining -= page_len;
        }

        Self {
            byte_length: size,
            buffers,
        }
    }

    pub fn set(&mut self, input: &[u8], offset: usize) {
        assert!(offset + input.len() <= self.byte_length);

        let mut remaining = input.len();
        let mut input_offset = 0;
        let mut page_idx = offset / PAGE_SIZE;
        let mut page_offset = offset % PAGE_SIZE;

        while remaining > 0 {
            let page = &mut self.buffers[page_idx];
            let len = min(PAGE_SIZE - page_offset, remaining);
            page[page_offset..page_offset + len]
                .copy_from_slice(&input[input_offset..input_offset + len]);

            remaining -= len;
            input_offset += len;
            page_idx += 1;
            page_offset = 0;
        }
    }

    pub fn slice(&self, from: usize, to: usize) -> Vec<u8> {
        assert!(to <= self.byte_length && from <= to);

        let mut result = vec![0u8; to - from];
        let mut remaining = to - from;
        let mut result_offset = 0;
        let mut page_idx = from / PAGE_SIZE;
        let mut page_offset = from % PAGE_SIZE;

        while remaining > 0 {
            let page = &self.buffers[page_idx];
            let len = min(PAGE_SIZE - page_offset, remaining);
            result[result_offset..result_offset + len]
                .copy_from_slice(&page[page_offset..page_offset + len]);

            remaining -= len;
            result_offset += len;
            page_idx += 1;
            page_offset = 0;
        }

        result
    }
}

impl Deref for BigBuffer {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        panic!("Direct deref not supported. Use `slice` instead.");
    }
}

impl DerefMut for BigBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        panic!("Direct deref_mut not supported. Use `set` instead.");
    }
}