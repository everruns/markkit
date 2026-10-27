//! Counting global allocator.
//!
//! Wraps the system allocator and tracks allocation count, bytes requested and
//! live/peak heap bytes with relaxed atomics. Overhead is a few atomic adds per
//! call, so timings stay comparable; allocation stats are still collected on a
//! separate pass (see `runner`) so they never skew the timed iterations.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

pub struct CountingAlloc;

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static LIVE: AtomicU64 = AtomicU64::new(0);
static PEAK: AtomicU64 = AtomicU64::new(0);

fn on_alloc(size: usize) {
    ALLOCS.fetch_add(1, Relaxed);
    BYTES.fetch_add(size as u64, Relaxed);
    let live = LIVE.fetch_add(size as u64, Relaxed) + size as u64;
    PEAK.fetch_max(live, Relaxed);
}

fn on_dealloc(size: usize) {
    LIVE.fetch_sub(size as u64, Relaxed);
}

// SAFETY: forwards every call to `System` unchanged; only bookkeeping is added.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            on_alloc(layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            on_alloc(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        on_dealloc(layout.size());
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            // A realloc counts as one allocation of the new size.
            on_dealloc(layout.size());
            on_alloc(new_size);
        }
        p
    }
}

/// Heap activity between [`Snapshot::start`] and [`Snapshot::finish`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllocStats {
    pub allocations: u64,
    pub allocated_bytes: u64,
    /// Peak live heap above the level at `start`.
    pub peak_bytes: u64,
}

pub struct Snapshot {
    allocs: u64,
    bytes: u64,
    live: u64,
}

impl Snapshot {
    /// Start a measurement window. Resets the peak to the current live level,
    /// so windows must not overlap (the harness is single-threaded).
    pub fn start() -> Self {
        let live = LIVE.load(Relaxed);
        PEAK.store(live, Relaxed);
        Self {
            allocs: ALLOCS.load(Relaxed),
            bytes: BYTES.load(Relaxed),
            live,
        }
    }

    pub fn finish(self) -> AllocStats {
        AllocStats {
            allocations: ALLOCS.load(Relaxed) - self.allocs,
            allocated_bytes: BYTES.load(Relaxed) - self.bytes,
            peak_bytes: PEAK.load(Relaxed).saturating_sub(self.live),
        }
    }
}

/// Process peak resident set size in bytes (Linux `VmHWM`), if available.
pub fn peak_rss_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmHWM:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}
