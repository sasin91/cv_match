//! llama: a GGUF model run by llama.cpp's llama-server, which the app starts itself (the
//! "llama_cpp" provider on the API key page). Its folder is `llama` next to the database unless
//! `--llama` names another; it holds llama-server.exe with its DLLs, unzipped from a llama.cpp
//! release, and models\*.gguf.
//!
//! The setup wizard (account::llama_setup) downloads both: the llama.cpp build for this
//! computer's card (download), and a model from the catalog that fits it.
//!
//! hardware reads the computer's memory and graphics card, model_choice weighs each model
//! against them, and llama_server starts the server with the chosen model. Its API is then the
//! openai adapter's local server, so every AI feature runs on it through the one code path.

pub mod catalog;
pub mod download;
pub mod gguf;
pub mod hardware;
pub mod llama_server;
pub mod model_choice;

use std::sync::Arc;

use crate::account::account_model::KeyFor;
use crate::llm::{LLAMA_CPP, LOCAL, Llm, LlmException};

/// The llama folder's name next to the database.
pub const FOLDER: &str = "llama";

/// The model the key's settings run on. For llama_cpp the app's llama-server is started (or kept)
/// with the key's model first; every other provider is Llm::use_key's.
pub async fn use_key(server: Option<&llama_server::LlamaServer>, key: &KeyFor) -> Result<Llm, LlmException> {
    if key.provider != LLAMA_CPP {
        return Llm::use_key(&key.provider, &key.api_key, &key.model, &key.base_url);
    }
    let server = server.ok_or_else(|| LlmException::new("The app was started without its llama folder."))?;
    let in_use = server.start(&key.model).await.map_err(LlmException::new)?;
    let mut llm = Llm::use_key(LOCAL, "", &in_use.endpoint.model, &in_use.endpoint.base_url)?;
    llm.server_in_use = Some(Arc::new(in_use));
    Ok(llm)
}
