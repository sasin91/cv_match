//! The job scan fits on a small stack, as Scan now runs it: on a tokio worker of its own.

use std::sync::Arc;

use jobfinder::account::key_vault::KeyVault;
use jobfinder::context::Context;
use jobfinder::db;
use jobfinder::job_scan::scan_service::{self, Started};
use jobfinder::server;
use jobfinder::taxonomy::Taxonomy;

/// A quarter of a tokio worker's own stack (2 MiB). Before the scan's work went on the heap
/// (running_work's module doc), a debug build's first poll of it took more than the whole 2 MiB.
const SMALL_STACK_BYTES: usize = 512 * 1024;

#[test]
fn a_scan_started_as_scan_now_starts_it_fits_on_a_small_stack() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .thread_stack_size(SMALL_STACK_BYTES)
        .enable_all()
        .build()
        .unwrap();
    let answer = runtime.block_on(async {
        let db = db::open_in_memory().await.unwrap();
        let user_id = server::local_user(&db).await.unwrap();
        let context = Arc::new(Context::new(
            db,
            Arc::new(Taxonomy::load()),
            user_id,
            KeyVault::in_memory(),
        ));
        // As scan_now spawns it: polled on the worker, not on this thread.
        context
            .running
            .clone()
            .spawn(scan_service::scan(Arc::clone(&context), Started::ByHand))
            .await
            .unwrap()
    });
    // No model is picked for reading job posts, so it stops at that, having started.
    assert!(answer.is_err());
}
