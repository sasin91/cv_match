//! Letting go of the models once they sit unused, so JobFinder left open in the background holds
//! neither llama-server's model (several GB on the graphics card) nor Laya's (1.7 GB of memory).
//! Each loads again by itself on the next call that needs it, which then waits a few seconds.
//! While the user plays (user_busy), llama-server stops as soon as no call needs it, so the game
//! has the graphics card.

use std::sync::Arc;
use std::time::Duration;

use crate::context::Context;
use crate::llama::llama_server::LlamaServer;
use crate::user_busy;

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
    if let Some(llama) = &context.llama {
        stop_llama_server_if_idle(llama).await;
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

/// Stops llama-server when it sat unused for UNLOAD_AFTER, or while the user plays once no call
/// needs it, and says so.
async fn stop_llama_server_if_idle(llama: &LlamaServer) {
    let playing = user_busy::why_busy();
    let idle_for = if playing.is_some() {
        Duration::ZERO
    } else {
        UNLOAD_AFTER
    };
    let Some(model) = llama.stop_if_idle(idle_for).await else {
        return;
    };
    match playing {
        Some(busy) => crate::say!(
            "llama: {busy}, so llama-server stopped ({model}) to leave the graphics card to the game. It starts again for the next call."
        ),
        None => crate::say!(
            "llama: {model} sat unused for {} minutes, so llama-server stopped. It starts again for the next call.",
            UNLOAD_AFTER.as_secs() / 60
        ),
    }
}
