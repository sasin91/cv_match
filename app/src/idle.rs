//! Letting go of the models once they sit unused, so JobFinder left open in the background holds
//! neither llama-server's model (several GB on the graphics card) nor Laya's (1.7 GB of memory).
//! Each loads again by itself on the next call that needs it, which then waits a few seconds.

use std::sync::Arc;
use std::time::Duration;

use crate::context::Context;

/// How long a model may sit unused before it is let go (five minutes): long enough that a few
/// matches in a row, or the posts of one scan, keep it loaded.
pub const UNLOAD_AFTER: Duration = Duration::from_secs(300);

/// How often the models are looked at (every half minute).
const CHECK_EVERY: Duration = Duration::from_secs(30);

/// Forever: every CHECK_EVERY, stops llama-server and lets Laya's model go when each sat unused
/// for UNLOAD_AFTER, and says so.
pub async fn unload_when_idle(context: Arc<Context>) {
    loop {
        tokio::time::sleep(CHECK_EVERY).await;
        unload_idle_models(&context).await;
    }
}

async fn unload_idle_models(context: &Context) {
    if let Some(llama) = &context.llama
        && let Some(model) = llama.stop_if_idle(UNLOAD_AFTER).await
    {
        crate::say!(
            "llama: {model} sat unused for {} minutes, so llama-server stopped. It starts again for the next call.",
            UNLOAD_AFTER.as_secs() / 60
        );
    }
    if let Some(laya) = &context.laya
        && laya.unload_if_idle(UNLOAD_AFTER)
    {
        crate::say!(
            "Laya sat unused for {} minutes, so its model was let go. It loads again for the next match.",
            UNLOAD_AFTER.as_secs() / 60
        );
    }
}
