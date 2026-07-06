//! Striped read-write lock manager for concurrent graph access.
//!
//! Uses `parking_lot::RwLock` for fast uncontended access and deadlock-free
//! lock ordering. Three lock domains exist:
//!
//! 1. **Block locks** — protect individual data/index blocks from concurrent
//!    mutation. Acquired exclusively when allocating chunks or writing block
//!    data; acquired shared when reading.
//!
//! 2. **Entity locks** — protect individual vertices and edges. Acquired
//!    exclusively during create/update/delete; shared during read.
//!
//! 3. **Metadata locks** — protect graph-level metadata (ID counters, etc.).
//!    Acquired exclusively during structural changes.
//!
//! # Deadlock prevention
//!
//! Locks must always be acquired in this order:
//!
//! ```text
//! metadata → block → vertex → edge
//! ```
//!
//! Violating this order may cause deadlocks. The `LockManager` does not
//! enforce this at compile time — it is a convention callers must follow.

use crate::storage::types::{BlockIdx, EdgeId, VertexId};
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Number of stripes for entity locks. Must be a power of two.
const STRIPE_COUNT: usize = 1024;

/// Number of stripes for block locks. Must be a power of two.
const BLOCK_STRIPE_COUNT: usize = 256;

/// A lock guard that is returned when acquiring a lock.
/// This prevents the lock from being released until the guard is dropped.
pub enum LockGuard<'a> {
    Read(RwLockReadGuard<'a, ()>),
    Write(RwLockWriteGuard<'a, ()>),
}

/// Manages all locks for a single graph instance.
pub struct LockManager {
    /// Block-level locks (striped by block_idx).
    block_stripes: Box<[RwLock<()>]>,
    /// Vertex-level locks (striped by vertex_id).
    vertex_stripes: Box<[RwLock<()>]>,
    /// Edge-level locks (striped by edge_id).
    edge_stripes: Box<[RwLock<()>]>,
    /// Metadata lock (graph-level structural changes).
    metadata_lock: RwLock<()>,
    /// Statistics.
    stats: LockStats,
}

/// Lock acquisition statistics (wrapped in a struct that can be cloned).
#[derive(Clone, Debug, Default)]
pub struct LockStats {
    pub block_reads: Arc<AtomicUsize>,
    pub block_writes: Arc<AtomicUsize>,
    pub vertex_reads: Arc<AtomicUsize>,
    pub vertex_writes: Arc<AtomicUsize>,
    pub edge_reads: Arc<AtomicUsize>,
    pub edge_writes: Arc<AtomicUsize>,
}

impl LockStats {
    fn new() -> Self {
        Self {
            block_reads: Arc::new(AtomicUsize::new(0)),
            block_writes: Arc::new(AtomicUsize::new(0)),
            vertex_reads: Arc::new(AtomicUsize::new(0)),
            vertex_writes: Arc::new(AtomicUsize::new(0)),
            edge_reads: Arc::new(AtomicUsize::new(0)),
            edge_writes: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl LockManager {
    /// Create a new lock manager with default stripe counts.
    pub fn new() -> Self {
        let block_stripes = (0..BLOCK_STRIPE_COUNT)
            .map(|_| RwLock::new(()))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let vertex_stripes = (0..STRIPE_COUNT)
            .map(|_| RwLock::new(()))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let edge_stripes = (0..STRIPE_COUNT)
            .map(|_| RwLock::new(()))
            .collect::<Vec<_>>()
            .into_boxed_slice();

        Self {
            block_stripes,
            vertex_stripes,
            edge_stripes,
            metadata_lock: RwLock::new(()),
            stats: LockStats::new(),
        }
    }

    // ── Block locks ─────────────────────────────────────────────────────────

    /// Acquire a shared (read) lock on a data/index block.
    #[inline]
    pub fn read_block(&self, block_idx: BlockIdx) -> LockGuard<'_> {
        self.stats.block_reads.fetch_add(1, Ordering::Relaxed);
        let stripe = self.block_stripe(block_idx);
        LockGuard::Read(stripe.read())
    }

    /// Acquire an exclusive (write) lock on a data/index block.
    #[inline]
    pub fn write_block(&self, block_idx: BlockIdx) -> LockGuard<'_> {
        self.stats.block_writes.fetch_add(1, Ordering::Relaxed);
        let stripe = self.block_stripe(block_idx);
        LockGuard::Write(stripe.write())
    }

    // ── Vertex locks ────────────────────────────────────────────────────────

    /// Acquire a shared (read) lock on a vertex.
    #[inline]
    pub fn read_vertex(&self, vertex_id: VertexId) -> LockGuard<'_> {
        self.stats.vertex_reads.fetch_add(1, Ordering::Relaxed);
        let stripe = self.vertex_stripe(vertex_id);
        LockGuard::Read(stripe.read())
    }

    /// Acquire an exclusive (write) lock on a vertex.
    #[inline]
    pub fn write_vertex(&self, vertex_id: VertexId) -> LockGuard<'_> {
        self.stats.vertex_writes.fetch_add(1, Ordering::Relaxed);
        let stripe = self.vertex_stripe(vertex_id);
        LockGuard::Write(stripe.write())
    }

    // ── Edge locks ──────────────────────────────────────────────────────────

    /// Acquire a shared (read) lock on an edge.
    #[inline]
    pub fn read_edge(&self, edge_id: EdgeId) -> LockGuard<'_> {
        self.stats.edge_reads.fetch_add(1, Ordering::Relaxed);
        let stripe = self.edge_stripe(edge_id);
        LockGuard::Read(stripe.read())
    }

    /// Acquire an exclusive (write) lock on an edge.
    #[inline]
    pub fn write_edge(&self, edge_id: EdgeId) -> LockGuard<'_> {
        self.stats.edge_writes.fetch_add(1, Ordering::Relaxed);
        let stripe = self.edge_stripe(edge_id);
        LockGuard::Write(stripe.write())
    }

    // ── Metadata lock ───────────────────────────────────────────────────────

    /// Acquire a shared (read) lock on graph metadata.
    #[inline]
    pub fn read_metadata(&self) -> LockGuard<'_> {
        LockGuard::Read(self.metadata_lock.read())
    }

    /// Acquire an exclusive (write) lock on graph metadata.
    #[inline]
    pub fn write_metadata(&self) -> LockGuard<'_> {
        LockGuard::Write(self.metadata_lock.write())
    }

    // ── Batch lock helpers ──────────────────────────────────────────────────

    /// Lock two blocks for a write operation (e.g., moving data from old to new).
    /// Acquires them in order to prevent deadlock.
    pub fn lock_two_blocks(&self, a: BlockIdx, b: BlockIdx) -> (LockGuard<'_>, LockGuard<'_>) {
        if a <= b {
            let ga = self.write_block(a);
            let gb = self.write_block(b);
            (ga, gb)
        } else {
            let gb = self.write_block(b);
            let ga = self.write_block(a);
            (ga, gb)
        }
    }

    /// Lock a vertex and a block (in order: block → vertex).
    pub fn lock_vertex_and_block(
        &self,
        vertex_id: VertexId,
        block_idx: BlockIdx,
    ) -> (LockGuard<'_>, LockGuard<'_>) {
        let bg = self.write_block(block_idx);
        let vg = self.write_vertex(vertex_id);
        (bg, vg)
    }

    // ── Statistics ──────────────────────────────────────────────────────────

    /// Get a snapshot of lock statistics.
    pub fn stats(&self) -> LockStatsSnapshot {
        LockStatsSnapshot {
            block_reads: self.stats.block_reads.load(Ordering::Relaxed),
            block_writes: self.stats.block_writes.load(Ordering::Relaxed),
            vertex_reads: self.stats.vertex_reads.load(Ordering::Relaxed),
            vertex_writes: self.stats.vertex_writes.load(Ordering::Relaxed),
            edge_reads: self.stats.edge_reads.load(Ordering::Relaxed),
            edge_writes: self.stats.edge_writes.load(Ordering::Relaxed),
        }
    }

    // ── Stripe computation ──────────────────────────────────────────────────

    #[inline]
    fn block_stripe(&self, block_idx: BlockIdx) -> &RwLock<()> {
        &self.block_stripes[(block_idx as usize) & (BLOCK_STRIPE_COUNT - 1)]
    }

    #[inline]
    fn vertex_stripe(&self, vertex_id: VertexId) -> &RwLock<()> {
        &self.vertex_stripes[(vertex_id as usize) & (STRIPE_COUNT - 1)]
    }

    #[inline]
    fn edge_stripe(&self, edge_id: EdgeId) -> &RwLock<()> {
        &self.edge_stripes[(edge_id as usize) & (STRIPE_COUNT - 1)]
    }
}

/// Snapshot of lock statistics at a point in time.
#[derive(Clone, Debug, Default)]
pub struct LockStatsSnapshot {
    pub block_reads: usize,
    pub block_writes: usize,
    pub vertex_reads: usize,
    pub vertex_writes: usize,
    pub edge_reads: usize,
    pub edge_writes: usize,
}
