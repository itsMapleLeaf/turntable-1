use super::loading::Loader;
use axum::response::IntoResponse;
use hyper::StatusCode;
use std::fmt::Display;
use thiserror::Error;

mod wavedistrict;
mod youtube;

#[derive(Debug, Clone)]
pub enum Input {
    WaveDistrict(wavedistrict::Track),
    YouTube(youtube::YouTubeVideo),
}

#[derive(Debug, Error)]
pub enum InputError {
    #[error("Input type is supported but resource was not found")]
    NotFound,

    #[error("Input did not match")]
    NoMatch,

    #[error("Unsupported input type")]
    UnsupportedType,

    #[error("Failed to fetch resource")]
    NetworkFailed,

    #[error("Resource is invalid")]
    Invalid,

    #[error(transparent)]
    Other(Box<dyn std::error::Error + Send>),

    #[error("An unknown error occurred")]
    Unknown,
}

impl Input {
    /// Returns the fingerprint used to check
    /// if this is already in cache
    pub fn fingerprint(&self) -> String {
        match self {
            Input::WaveDistrict(t) => t.fingerprint(),
            Input::YouTube(v) => v.fingerprint(),
        }
    }

    pub fn parse(str: &str) -> Result<Self, InputError> {
        let predicates = [
            |url| youtube::YouTubeVideo::from_url(url).map(Self::YouTube),
            |url| wavedistrict::Track::from_url(url).map(Self::WaveDistrict),
        ];

        predicates
            .into_iter()
            .map(|f| f(str))
            .find_map(|r| match r {
                Err(InputError::NoMatch) => None,
                x => Some(x),
            })
            .unwrap_or(Err(InputError::UnsupportedType))
    }

    pub fn loader(&self) -> Result<Box<dyn Loader>, InputError> {
        match self {
            Input::YouTube(video) => video.loader(),
            Input::WaveDistrict(track) => track.loader(),
        }
    }

    pub fn duration(&self) -> f32 {
        match self {
            Input::WaveDistrict(x) => x.duration(),
            Input::YouTube(x) => x.duration(),
        }
    }
}

impl Display for Input {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            Input::WaveDistrict(x) => x.fmt(f),
            Input::YouTube(x) => x.fmt(f),
        }
    }
}

impl From<&Input> for String {
    fn from(x: &Input) -> Self {
        x.to_string()
    }
}

impl IntoResponse for InputError {
    fn into_response(self) -> axum::response::Response {
        let status = match self {
            InputError::NotFound => StatusCode::NOT_FOUND,
            InputError::NoMatch => StatusCode::BAD_REQUEST,
            InputError::UnsupportedType => StatusCode::BAD_REQUEST,
            InputError::Invalid => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };

        (status, self.to_string()).into_response()
    }
}