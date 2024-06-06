use std::{error::Error, io::SeekFrom};

use async_trait::async_trait;
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt},
    sync::Mutex,
};

use crate::{IntoLoadable, Loadable, LoaderLength, ReadResult};
pub struct LoadableFile(Mutex<File>);

#[async_trait]
impl Loadable for LoadableFile {
    async fn read(&self, buf: &mut [u8]) -> Result<ReadResult, Box<dyn Error>> {
        let mut file = self.0.lock().await;
        let result = file.read(buf).await?;

        if result == 0 {
            return Ok(ReadResult::End(0));
        }

        Ok(ReadResult::More(result))
    }

    async fn length(&self) -> Option<LoaderLength> {
        let file = self.0.lock().await;

        file.metadata()
            .await
            .ok()
            .map(|meta| meta.len())
            .map(|length| LoaderLength::Bytes(length as usize))
    }

    async fn seek(&self, seek: SeekFrom) -> Result<usize, Box<dyn Error>> {
        let mut file = self.0.lock().await;
        let result = file.seek(seek).await?;

        Ok(result as usize)
    }
}

impl IntoLoadable for File {
    type Output = LoadableFile;

    fn into_loadable(self) -> Self::Output {
        LoadableFile(Mutex::new(self))
    }
}

#[cfg(test)]
pub mod tests {
    use std::path::PathBuf;

    use tokio::fs::File;

    pub fn test_file_path() -> PathBuf {
        let root = env!("CARGO_MANIFEST_DIR");
        let mut path = PathBuf::from(root);

        path.pop();
        path.push("assets");
        path.push("deep_blue.flac");

        path
    }

    pub async fn test_file() -> File {
        File::open(test_file_path()).await.unwrap()
    }
}
