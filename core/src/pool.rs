//! HEL1-pinned thread pool â€” exactly 12 threads on 6 P-cores (SMT).
//!
//! Determinism contract:
//! 1. Work is partitioned by **index** before threads spawn â€” each worker owns
//!    a contiguous `[start, end)` range. No stealing, no completion-order writes.
//! 2. Results land in `results[i]` where `i` is the *input index*, never the
//!    completion slot.
//! 3. Merging is done in **worker-index order** in the main thread, so byte
//!    output is identical between 1-thread and 12-thread runs.

use std::sync::{Arc, Mutex};
use std::thread;

/// A fixed-size thread pool. `Pool::hel1()` spawns 12 threads (HEL1 cpuset),
/// `Pool::hel1_physical()` spawns 6 (no SMT). Benchmark both.
#[derive(Debug)]
pub struct Pool {
    num_threads: usize,
}

impl Pool {
    pub fn new(num_threads: usize) -> Self {
        assert!(
            num_threads > 0 && num_threads <= 12,
            "HEL1 cpuset is 12 threads"
        );
        Self { num_threads }
    }

    /// HEL1 default: 12 threads (6 P-core Ã— 2 SMT).
    pub fn hel1() -> Self {
        Self::new(12)
    }

    /// 6 workers (one per physical core, no SMT) â€” often faster due to
    /// reduced cache contention. Benchmark both.
    pub fn hel1_physical() -> Self {
        Self::new(6)
    }

    pub fn num_threads(&self) -> usize {
        self.num_threads
    }

    /// Parallel map preserving **input order** in the output.
    ///
    /// Partitions `items` into `num_threads` contiguous chunks, one per
    /// worker, computes `f` on each item, and collects into a pre-allocated
    /// `Vec<R>` indexed by input position. Result order is deterministic
    /// regardless of thread scheduling.
    pub fn par_map<T, R, F>(&self, items: Vec<T>, f: F) -> Vec<R>
    where
        T: Send + 'static,
        R: Send + 'static,
        F: Fn(T) -> R + Send + Sync + 'static,
    {
        let n = items.len();
        if n == 0 {
            return Vec::new();
        }
        if n == 1 || self.num_threads == 1 {
            return items.into_iter().map(&f).collect();
        }

        let mut items = items;
        let chunk = n.div_ceil(self.num_threads);
        let f = Arc::new(f);

        // Pre-split into owned chunks (main thread, before spawn).
        let mut chunks: Vec<Vec<T>> = Vec::with_capacity(self.num_threads);
        while !items.is_empty() {
            let take = chunk.min(items.len());
            chunks.push(items.drain(..take).collect());
        }

        // results[t] = the output for chunks[t]; collected in worker order.
        let results: Arc<Mutex<Vec<Option<Vec<R>>>>> =
            Arc::new(Mutex::new((0..chunks.len()).map(|_| None).collect()));
        let mut handles = Vec::new();

        for (t, chunk_items) in chunks.into_iter().enumerate() {
            let f = Arc::clone(&f);
            let results = Arc::clone(&results);
            let h = thread::spawn(move || {
                let out: Vec<R> = chunk_items.into_iter().map(|x| f(x)).collect();
                let mut guard = results.lock().unwrap();
                guard[t] = Some(out);
            });
            handles.push(h);
        }

        // Join in worker-index order (deterministic collection).
        for h in handles {
            h.join().expect("worker thread panicked");
        }
        let mut guard = results.lock().unwrap();
        let mut final_results: Vec<R> = Vec::with_capacity(n);
        for slot in guard.iter_mut() {
            final_results.append(slot.as_mut().expect("worker slot must be filled"));
        }
        final_results
    }

    /// Parallel for-each over deterministic byte ranges of a flat buffer.
    /// `is_boundary(offset)` must return `true` only at true logical row
    /// boundaries (never inside a quoted field on Track C). Each worker
    /// gets `[start, end)` where the byte at `start` begins a row.
    pub fn par_chunks<F>(
        &self,
        data_len: usize,
        is_boundary: impl Fn(usize) -> bool + Send + Sync + 'static,
        f: F,
    ) where
        F: Fn(usize, usize) + Send + Sync + 'static,
    {
        if data_len == 0 {
            return;
        }
        let bounds = crate::buf_reader::chunk_boundaries(data_len, self.num_threads, is_boundary);
        let bounds = Arc::new(bounds);
        let f = Arc::new(f);
        let mut handles = Vec::new();
        for w in 0..bounds.len().saturating_sub(1) {
            let (s, e) = (bounds[w], bounds[w + 1]);
            if s == e {
                continue;
            }
            let f = Arc::clone(&f);
            let h = thread::spawn(move || f(s, e));
            handles.push(h);
        }
        for h in handles {
            h.join().expect("chunk worker panicked");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn par_map_preserves_input_order() {
        let pool = Pool::new(12);
        let v: Vec<usize> = (0..1000).collect();
        let out = pool.par_map(v, |x| x * 3 + 7);
        assert_eq!(out, (0..1000).map(|x| x * 3 + 7).collect::<Vec<_>>());
    }

    #[test]
    fn par_map_threads_actually_run() {
        // Prove workers really are used, not silently degraded to sequential.
        let counter = Arc::new(AtomicUsize::new(0));
        let pool = Pool::hel1();
        let c2 = Arc::clone(&counter);
        let _ = pool.par_map((0..2000usize).collect::<Vec<_>>(), move |x| {
            c2.fetch_add(1, Ordering::SeqCst);
            x
        });
        assert_eq!(counter.load(Ordering::SeqCst), 2000);
    }

    #[test]
    fn par_chunks_covers_all_bytes() {
        let pool = Pool::new(6);
        let covered = Arc::new(Mutex::new(vec![false; 1000]));
        let cov2 = Arc::clone(&covered);
        pool.par_chunks(
            1000,
            |_| true, // every offset is a boundary for this test
            move |s, e| {
                let mut g = cov2.lock().unwrap();
                for i in s..e {
                    g[i] = true;
                }
            },
        );
        assert!(covered.lock().unwrap().iter().all(|&b| b));
    }

    #[test]
    fn par_chunks_respects_boundaries() {
        // boundaries only at multiples of 100; chunks must snap to them
        let pool = Pool::new(4);
        let ranges = Arc::new(Mutex::new(Vec::new()));
        let r2 = Arc::clone(&ranges);
        pool.par_chunks(
            1000,
            |off| off % 100 == 0,
            move |s, _e| {
                r2.lock().unwrap().push(s);
            },
        );
        let starts = ranges.lock().unwrap();
        for &s in starts.iter() {
            assert_eq!(s % 100, 0, "chunk start {s} is not on a row boundary");
        }
    }
}
