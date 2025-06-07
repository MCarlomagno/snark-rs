use std::collections::{HashMap, VecDeque};

use anyhow::bail;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use std::io::{self, SeekFrom};
use std::path::Path;
use std::sync::{Arc};
use tokio::sync::{oneshot, Mutex};
use tokio::fs::{File, OpenOptions};

pub struct LoadRequest {
    pub page: u64,
    pub sender: oneshot::Sender<Arc<Mutex<Page>>>,
}

pub struct Page {
    pub buffer: Vec<u8>,
    pub pending_ops: usize,
    pub loading: Option<Vec<oneshot::Sender<Arc<Mutex<Page>>>>>,
    pub writing: bool,
    pub dirty: bool,
    pub size: usize,
}

pub struct FastFile {
    pub file: Arc<Mutex<File>>,
    pub file_size: u64,
    pub cache_size: usize,
    pub page_size: usize,
    pub file_name: String,

    pub pos: u64,
    pub pending_close: bool,
    pub max_pages_loaded: usize,
    pub pages: HashMap<u64, Arc<Mutex<Page>>>,
    pub pending_loads: VecDeque<LoadRequest>,
    pub history: HashMap<u64, Vec<String>>,
    pub log_history: bool,
    pub reading: bool,
}

impl FastFile {
    pub async fn read(&mut self, len: usize, pos: Option<u64>) -> anyhow::Result<Vec<u8>> {
        let mut buffer = vec![0u8; len];
        self.read_to_buffer(&mut buffer, 0, len, pos).await?;
        Ok(buffer)
    }

    pub async fn read_to_buffer(
        &mut self,
        buff_dst: &mut [u8],
        offset: usize,
        len: usize,
        pos: Option<u64>,
    ) -> anyhow::Result<()> {
        if len == 0 {
            return Ok(());
        }

        let pos = pos.unwrap_or(self.pos);
        self.pos = pos + len as u64;

        if self.pending_close {
            anyhow::bail!("Reading a closing file");
        }

        if len as f64 > (self.page_size * self.max_pages_loaded) as f64 * 0.8 {
            let cache_size = ((len as f64) * 1.1).ceil() as usize;
            self.max_pages_loaded = cache_size / self.page_size + 1;
        }

        let first_page = pos / self.page_size as u64;
        let last_page = (pos + len as u64 - 1) / self.page_size as u64;

        // pre-load all pages
        let mut page_promises = Vec::new();
        for page_num in first_page..=last_page {
            page_promises.push(self.load_page(page_num));
        }

        self.trigger_load().await?;

        let mut p = first_page;
        let mut o = (pos % self.page_size as u64) as usize;
        let mut r = if pos + len as u64 > self.file_size {
            len - ((pos + len as u64 - self.file_size) as usize)
        } else {
            len
        };

        while r > 0 {
            let idx = (p - first_page) as usize;
            let receiver = &mut page_promises[idx];
            let page = receiver.await?;
            let mut page = page.lock().await;

            let bytes_to_copy = if o + r > self.page_size {
                self.page_size - o
            } else {
                r
            };

            let start = offset + len - r;
            buff_dst[start..start + bytes_to_copy]
                .copy_from_slice(&page.buffer[o..o + bytes_to_copy]);
            page.pending_ops -= 1;

            r -= bytes_to_copy;
            p += 1;
            o = 0;
        }

        self.pos = pos + len as u64;
        Ok(())
    }

    pub fn load_page(&mut self, page_num: u64) -> oneshot::Receiver<Arc<Mutex<Page>>> {
        let (sender, receiver) = oneshot::channel();

        self.pending_loads.push_back(LoadRequest {
            page: page_num,
            sender,
        });

        self.status_page("After Load request", page_num);
        receiver
    }

    pub async fn trigger_load(&mut self) -> anyhow::Result<()> {
        loop {
            if self.reading || self.pending_loads.is_empty() {
                return Ok(());
            }
    
            self.reading = true;
    
            // Step 1: Identify deletable pages
            let mut deletable_pages = vec![];
            for (&page_num, page) in &self.pages {
                let page = page.lock().await;
                if !page.dirty && page.pending_ops == 0 && !page.writing && page.loading.is_none() {
                    deletable_pages.push(page_num);
                }
            }
    
            let mut free_pages = self.max_pages_loaded as isize - self.pages.len() as isize;
            let mut ops = vec![];
    
            // Step 2: Load or fulfill as many pages as we can
            while let Some(load) = self.pending_loads.front() {
                let page_num = load.page;
    
                let already_loaded = self.pages.contains_key(&page_num);
                let can_load = free_pages > 0 || !deletable_pages.is_empty();
    
                if !already_loaded && !can_load {
                    break;
                }
    
                let load = self.pending_loads.pop_front().unwrap();
    
                if let Some(page_arc) = self.pages.get(&load.page) {
                    let page = page_arc.clone();
                    let mut page_guard = page.lock().await;
                    page_guard.pending_ops += 1;
    
                    if let Some(load_list) = &mut page_guard.loading {
                        load_list.push(load.sender);
                    } else {
                        let _ = load.sender.send(page.clone());
                    }
                } else {
                    // Evict a deletable page if needed
                    if free_pages > 0 {
                        free_pages -= 1;
                    } else if let Some(evict_page) = deletable_pages.pop() {
                        self.pages.remove(&evict_page);
                    }
    
                    let page = Arc::new(Mutex::new(Page {
                        buffer: vec![0u8; self.page_size],
                        pending_ops: 1,
                        loading: Some(vec![load.sender]),
                        writing: false,
                        dirty: false,
                        size: 0,
                    }));
    
                    self.pages.insert(load.page, page.clone());
    
                    // Spawn task to load the page from disk
                    let file = self.file.clone();
                    let page_size = self.page_size;
                    let page_num = load.page;
                    let page_clone = page.clone();
    
                    ops.push(tokio::spawn(async move {
                        let mut file = file.lock().await;
                        let offset = page_num * page_size as u64;
                        let mut buffer = vec![0u8; page_size];
                        file.seek(SeekFrom::Start(offset)).await.ok()?;
                        let size = file.read(&mut buffer).await.ok()?;
    
                        let mut p = page_clone.lock().await;
                        p.buffer = buffer;
                        p.size = size;
                        let loading = p.loading.take();
    
                        drop(p); // drop lock before notifying
    
                        if let Some(waiters) = loading {
                            for sender in waiters {
                                let _ = sender.send(page_clone.clone());
                            }
                        }
    
                        Some(())
                    }));
                }
            }
    
            // Step 3: Wait for all spawned I/O ops
            for op in ops {
                let _ = op.await;
            }
    
            self.reading = false;
    
            // Step 4: Loop again if more pending loads
            if self.pending_loads.is_empty() {
                break;
            }
        }
    
        Ok(())
    }

    async fn status_page(&mut self, label: &str, page: u64) {
        if !self.log_history {
            return;
        }

        let mut log_entry = vec![format!("=={} {}", label, page)];

        let mut pending = String::new();
        for (i, req) in self.pending_loads.iter().enumerate() {
            if req.page == page {
                pending.push_str(&format!(" {}", i));
            }
        }
        if !pending.is_empty() {
            log_entry.push(format!("Pending loads:{}", pending));
        }

        if let Some(page_data) = self.pages.get(&page) {
            log_entry.push("Loaded".to_string());
            let page = page_data.lock().await;
            log_entry.push(format!("pendingOps: {}", page.pending_ops));
            // Add dummy flags if desired: loading, writing, dirty, etc.
        }

        log_entry.push("==".to_string());
        self.history
            .entry(page)
            .or_default()
            .push(log_entry.join(" | "));
    }

    pub async fn read_ule32(&mut self, pos: u64) -> anyhow::Result<u32> {
        let buf = self.read(4, Some(pos)).await?;

        if buf.len() != 4 {
            bail!("Unexpected EOF when reading u32 at pos {}", pos);
        }

        Ok(u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]))
    }

    pub async fn read_ule64(&mut self, pos: u64) -> anyhow::Result<u64> {
        let buf = self.read(8, Some(pos)).await?;

        if buf.len() != 8 {
            bail!("Unexpected EOF when reading u64 at pos {}", pos);
        }

        Ok(u64::from_le_bytes([
            buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
        ]))
    }
}

pub async fn open_file<P: AsRef<Path>>(
    file_name: P,
    open_flags: i32,
    cache_size: Option<usize>,
    page_size: Option<usize>,
) -> io::Result<FastFile> {
    const DEFAULT_CACHE_SIZE: usize = 1 << 16;
    const DEFAULT_PAGE_SIZE: usize = 1 << 13;
    let cache_size = cache_size.unwrap_or(DEFAULT_CACHE_SIZE);
    let page_size = page_size.unwrap_or(DEFAULT_PAGE_SIZE);

    let mut options = OpenOptions::new();

    if open_flags & libc::O_RDWR != 0 {
        options.read(true).write(true);
    } else if open_flags & libc::O_RDONLY != 0 {
        options.read(true);
    } else {
        options.write(true);
    }

    if open_flags & libc::O_CREAT != 0 {
        options.create(true);
    }
    if open_flags & libc::O_TRUNC != 0 {
        options.truncate(true);
    }

    options.custom_flags(open_flags & libc::O_EXCL); // Only keep O_EXCL here

    let file = options.open(&file_name).await?;
    let metadata = file.metadata().await?;
    let file_size = metadata.len();

    Ok(FastFile {
        file: Arc::new(Mutex::new(file)),
        file_size,
        cache_size,
        page_size,
        file_name: file_name.as_ref().to_string_lossy().to_string(),

        // Add remaining fields
        pos: 0,
        pending_close: false,
        max_pages_loaded: cache_size / page_size,
        pages: HashMap::new(),
        pending_loads: VecDeque::new(),
        history: HashMap::new(),
        log_history: false,
        reading: false,
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write, path::PathBuf};

    use tokio::io::AsyncWriteExt;

    use super::*;

    #[tokio::test]
    async fn test_open_fixture_readonly() {
        let path = PathBuf::from("src/fixtures/pot28.ptau");
        let file = open_file(&path, libc::O_RDONLY, None, None).await.expect("should open dummy file");
        assert!(
            file.file_size > 0,
            "Expected file size to be greater than 0"
        );
    }

    #[tokio::test]
    async fn test_open_existing_file_readonly() {
        let path = PathBuf::from("src/fixtures/dummy.ptau");

        // Ensure the dummy file exists
        fs::create_dir_all("src/fixtures").unwrap();
        let mut file = File::create(&path).await.unwrap();
        file.write_all(b"0123456789abcdef").await.unwrap();

        let fast_file =
            open_file(&path, libc::O_RDONLY, None, None).await.expect("Failed to open dummy file");

        assert_eq!(fast_file.file_name, path.to_string_lossy());
        assert_eq!(fast_file.file_size, 16);
        assert_eq!(fast_file.cache_size, 1 << 16);
        assert_eq!(fast_file.page_size, 1 << 13);
        assert!(!fast_file.pending_close);
        assert_eq!(fast_file.pos, 0);
        assert_eq!(
            fast_file.max_pages_loaded,
            fast_file.cache_size / fast_file.page_size
        );

        // Clean up
        fs::remove_file(&path).unwrap();
    }

    #[tokio::test]
    async fn test_open_temp_file_write_create_trunc() {
        let path = PathBuf::from("src/fixtures/temp.ptau");

        // Clean up if it exists
        let _ = fs::remove_file(&path);

        let fast_file = open_file(
            &path,
            libc::O_RDWR | libc::O_CREAT | libc::O_TRUNC | libc::O_EXCL,
            Some(8192),
            Some(4096),
        )
        .await
        .expect("Failed to open temp file");

        assert_eq!(fast_file.cache_size, 8192);
        assert_eq!(fast_file.page_size, 4096);
        assert_eq!(fast_file.max_pages_loaded, 2);
        assert_eq!(fast_file.pos, 0);

        // Clean up
        fs::remove_file(&path).unwrap();
    }
}
