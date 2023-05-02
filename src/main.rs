use std::sync::Arc;

use audio::AudioSystem;
use colored::Colorize;
use db::Database;
use log::{error, info};
use server::ws::WebSocketManager;
use thiserror::Error;
use tokio::runtime::{self, Runtime};

use crate::logging::LogColor;

mod audio;
mod db;
mod http;
mod ingest;
mod logging;
mod server;
mod util;

pub struct Vinyl {
    db: Arc<Database>,
    // This is temporary for now, since Rooms will have their own audio system
    audio: Arc<AudioSystem>,
    websockets: Arc<WebSocketManager>,

    runtime: Runtime,
}

#[derive(Clone)]
pub struct VinylContext {
    pub db: Arc<Database>,
    pub audio: Arc<AudioSystem>,
    pub websockets: Arc<WebSocketManager>,
}

#[derive(Debug, Error)]
enum VinylError {
    #[error("Could not initialize database: {0}")]
    Database(#[from] surrealdb::Error),

    #[error("Fatal error: {0}")]
    Fatal(String),
}

impl Vinyl {
    fn new() -> Result<Self, VinylError> {
        info!("Building async runtime...");
        let main_runtime = runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("vinyl-async")
            .build()
            .map_err(|e| VinylError::Fatal(e.to_string()))?;

        info!("Connecting to database...");
        let database = main_runtime.block_on(db::connect())?;

        Ok(Self {
            db: database.into(),
            audio: AudioSystem::new(),
            websockets: WebSocketManager::new(),
            runtime: main_runtime,
        })
    }

    fn run(&self) {
        audio::spawn_audio_thread(self.audio.clone());

        self.runtime
            .block_on(async move { server::run_server(self.context()).await });
    }

    fn context(&self) -> VinylContext {
        VinylContext {
            db: self.db.clone(),
            audio: self.audio.clone(),
            websockets: self.websockets.clone(),
        }
    }
}

impl VinylError {
    fn hint(&self) -> String {
        match self {
            VinylError::Database(_) => "This is a database error. Make sure the SurrealDB instance is properly installed and running, then try again.".to_string(),
            VinylError::Fatal(_) => "This error is fatal, and should not happen.".to_string(),
        }
    }
}

fn main() {
    logging::init_logger();

    match Vinyl::new() {
        Ok(vinyl) => {
            info!("Initialized successfully.");
            vinyl.run();
        }
        Err(error) => {
            error!("{} Read the error below to troubleshoot the issue. If you think this might be a bug, please report it by making a GitHub issue.", "Vinyl failed to start!".bold().color(LogColor::Red));
            error!("{}", error);
            error!(
                "{}",
                format!("Hint: {}", error.hint())
                    .color(LogColor::Dimmed)
                    .italic()
            );
        }
    }
}
