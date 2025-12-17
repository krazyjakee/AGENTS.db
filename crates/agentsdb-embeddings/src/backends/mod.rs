//! Embedding backend implementations.
//!
//! This module provides embedding backends for various providers and local inference engines.
//! Each backend is feature-gated and can be enabled independently.
//!
//! # Available Backends
//!
//! ## Cloud API Backends
//! - `openai` - OpenAI embeddings API
//! - `voyage` - Voyage AI embeddings API
//! - `cohere` - Cohere embeddings API
//! - `anthropic` - Anthropic embeddings API
//! - `bedrock` - AWS Bedrock embeddings
//! - `gemini` - Google Gemini embeddings API
//!
//! ## Local Inference Backends
//! - `candle` - Candle-based BERT inference (CPU/GPU)
//! - `ort` - ONNX Runtime via FastEmbed (CPU optimized)

#![cfg_attr(
    not(any(
        feature = "openai",
        feature = "voyage",
        feature = "cohere",
        feature = "anthropic",
        feature = "bedrock",
        feature = "gemini",
        feature = "candle",
        feature = "ort"
    )),
    allow(dead_code, unused_imports)
)]

// Submodule declarations
mod common;

#[cfg(feature = "candle")]
mod candle;

#[cfg(feature = "openai")]
mod openai;

#[cfg(feature = "voyage")]
mod voyage;

#[cfg(feature = "cohere")]
mod cohere;

#[cfg(feature = "ort")]
mod fastembed;

#[cfg(feature = "anthropic")]
mod anthropic;

#[cfg(feature = "bedrock")]
mod bedrock;

#[cfg(feature = "gemini")]
mod gemini;

// Public re-exports
#[cfg(feature = "candle")]
pub use candle::local_candle_embedder;

#[cfg(feature = "openai")]
pub use openai::openai_embedder;

#[cfg(feature = "voyage")]
pub use voyage::voyage_embedder;

#[cfg(feature = "cohere")]
pub use cohere::cohere_embedder;

#[cfg(feature = "ort")]
pub use fastembed::local_fastembed_embedder;

#[cfg(feature = "anthropic")]
pub use anthropic::anthropic_embedder;

#[cfg(feature = "bedrock")]
pub use bedrock::bedrock_embedder;

#[cfg(feature = "gemini")]
pub use gemini::gemini_embedder;
