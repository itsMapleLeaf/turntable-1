//! The ingestion is responsible for the process of loading inputs into a sink.

use async_trait::async_trait;
use std::{error::Error, sync::Arc};

use crate::Config;

mod loading;
mod sink;

pub use loading::*;
pub use sink::*;

/// Represents a type that can ingest sources and load samples into a [Sink].
///
/// This is used for the creation of sinks and loading of external sources.
#[async_trait]
// Todo: Rename this to just Ingestion
pub trait Ingestion {
    async fn new(config: Config) -> Self;

    /// Ingests a new source, returning a sink that can be used to play the source.
    async fn ingest<L>(&self, input: L) -> Result<Arc<Sink>, Box<dyn Error>>
    where
        L: Loadable;

    /// Requests the pipeline to start loading samples into a sink.
    ///
    /// Note: This function must not be called on the playback thread.
    async fn request_load(&self, sink_id: SinkId, offset: usize, amount: usize);
}
