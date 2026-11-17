//! The local AI: the model files, the runner process (lastbook-llm, which speaks only through pipes), whether a model fits
//! this computer, the prompt formats of the model families, and the assistant that answers from the library with citations.
//! Ported from the original Lastbook's llm_local.py and assistant.py.

pub mod gguf;
pub mod format;
pub mod local;
pub mod assistant;
pub mod chat;
