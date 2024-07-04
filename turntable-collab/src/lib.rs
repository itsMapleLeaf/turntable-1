mod auth;
mod db;
mod events;
mod input;
mod queues;
mod rooms;
mod track;
mod util;

use auth::Auth;
use rooms::{RoomId, RoomManager};
use std::sync::Arc;

pub use auth::{AuthError, Credentials, NewPlainUser};
pub use db::*;
pub use input::*;
pub use queues::*;
pub use rooms::{Room, RoomConnection, RoomConnectionHandle, RoomError, RoomState};
pub use track::*;

use turntable_core::{ArcedStore, Config, Pipeline, PlayerId};
use turntable_impls::SymphoniaIngestion;

pub type CollabPipeline = Pipeline<SymphoniaIngestion>;
pub type CollabDatabase = PgDatabase;

/// The turntable collab system, facilitating room management, authentication, and more.
pub struct Collab {
    pipeline: Arc<CollabPipeline>,
    database: Arc<CollabDatabase>,

    pub auth: Auth<CollabDatabase>,
    pub rooms: RoomManager,
}

/// A type passed to various components of the collab system, to access state, emit events, and dispatch actions.
pub struct CollabContext {
    pub pipeline: Arc<CollabPipeline>,
    pub database: Arc<CollabDatabase>,

    pub rooms: ArcedStore<RoomId, Room>,
}

impl Collab {
    pub async fn new(config: Config, database_url: &str) -> Self {
        let database = Arc::new(
            CollabDatabase::new(database_url)
                .await
                .expect("database is created"),
        );
        let pipeline = Arc::new(CollabPipeline::new(config));

        let context = CollabContext {
            database: database.clone(),
            pipeline: pipeline.clone(),

            rooms: Default::default(),
        };

        let room_manager = RoomManager::new(&context);
        let auth = Auth::new(&database);

        let new = Self {
            auth,
            pipeline,
            database,
            rooms: room_manager,
        };

        new.init().await;
        new
    }

    /// Must be called after creation
    async fn init(&self) {
        self.rooms.restore().await.expect("rooms are restored");
    }
}

impl Clone for CollabContext {
    fn clone(&self) -> Self {
        Self {
            database: self.database.clone(),
            pipeline: self.pipeline.clone(),
            rooms: self.rooms.clone(),
        }
    }
}

impl CollabContext {
    /// Gets a room by its player id if it exists and is active
    pub fn room_by_player_id(&self, player_id: PlayerId) -> Option<Arc<Room>> {
        self.rooms
            .iter()
            .find(|r| r.player().ok().filter(|p| p.id == player_id).is_some())
            .map(|r| r.clone())
    }
}
