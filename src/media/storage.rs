use std::path::PathBuf;
use thiserror::Error;
use tokio::fs;

use crate::config::Config;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("storage client error: {0}")]
    Client(String),
}

#[derive(Clone)]
pub struct LocalStorage {
    root: PathBuf,
    app_origin: String,
}

impl LocalStorage {
    pub fn new(root: PathBuf, app_origin: String) -> Self {
        Self { root, app_origin }
    }

    fn resolve_safe_path(&self, key: &str) -> Result<PathBuf, StorageError> {
        let p = std::path::Path::new(key);
        if p.is_absolute() {
            return Err(StorageError::Client("Absolute paths are not allowed".into()));
        }
        for component in p.components() {
            match component {
                std::path::Component::Normal(_) => {}
                _ => return Err(StorageError::Client("Path traversal components not allowed".into())),
            }
        }
        let canonical_root = self.root.canonicalize().unwrap_or_else(|_| self.root.clone());
        let full_path = self.root.join(p);
        let canonical_target = full_path.canonicalize().map_err(StorageError::Io)?;
        if !canonical_target.starts_with(&canonical_root) {
            return Err(StorageError::Client("Escaping storage root is forbidden".into()));
        }
        Ok(canonical_target)
    }

    pub async fn put_object(
        &self,
        key: &str,
        _content_type: &str,
        data: &[u8],
    ) -> Result<String, StorageError> {
        let p = std::path::Path::new(key);
        if p.is_absolute() {
            return Err(StorageError::Client("Absolute paths are not allowed".into()));
        }
        for component in p.components() {
            match component {
                std::path::Component::Normal(_) => {}
                _ => return Err(StorageError::Client("Path traversal components not allowed".into())),
            }
        }
        let dest = self.root.join(key);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&dest, data).await?;
        let origin = self.app_origin.trim_end_matches('/');
        Ok(format!("{}/api/v1/media/{}", origin, key))
    }

    pub async fn open_file(&self, key: &str) -> Result<(String, fs::File), StorageError> {
        let path = self.resolve_safe_path(key)?;
        let file = fs::File::open(&path).await?;
        let mime = mime_guess(key);
        Ok((mime.to_string(), file))
    }

    pub async fn get_object(&self, key: &str) -> Result<(String, Vec<u8>), StorageError> {
        let path = self.resolve_safe_path(key)?;
        let data = fs::read(&path).await?;
        let mime = mime_guess(key);
        Ok((mime.to_string(), data))
    }

    pub async fn delete_object(&self, key: &str) -> Result<(), StorageError> {
        if let Ok(path) = self.resolve_safe_path(key) {
            if path.exists() {
                fs::remove_file(&path).await?;
                if let Some(parent) = path.parent() {
                    let canonical_root = self.root.canonicalize().unwrap_or_else(|_| self.root.clone());
                    if parent != canonical_root && parent.starts_with(&canonical_root) {
                        let _ = fs::remove_dir(parent).await;
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct TigrisStorage {
    client: aws_sdk_s3::Client,
    bucket: String,
    endpoint: String,
    public_url_prefix: Option<String>,
}

impl TigrisStorage {
    pub fn new(
        endpoint: String,
        bucket: String,
        access_key: String,
        secret_key: String,
        region: String,
        public_url_prefix: Option<String>,
    ) -> Self {
        let credentials = aws_sdk_s3::config::Credentials::new(
            access_key,
            secret_key,
            None,
            None,
            "tigris",
        );
        let conf = aws_sdk_s3::config::Builder::new()
            .endpoint_url(&endpoint)
            .region(aws_sdk_s3::config::Region::new(region))
            .credentials_provider(credentials)
            .force_path_style(true)
            .behavior_version_latest()
            .build();
        let client = aws_sdk_s3::Client::from_conf(conf);
        Self {
            client,
            bucket,
            endpoint,
            public_url_prefix,
        }
    }

    pub async fn put_object(
        &self,
        key: &str,
        content_type: &str,
        data: &[u8],
    ) -> Result<String, StorageError> {
        let body = aws_sdk_s3::primitives::ByteStream::from(data.to_vec());
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type(content_type)
            .body(body)
            .send()
            .await
            .map_err(|e| StorageError::Client(e.to_string()))?;

        if let Some(ref prefix) = self.public_url_prefix {
            Ok(format!("{}/{}", prefix.trim_end_matches('/'), key))
        } else {
            let ep = self.endpoint.trim_end_matches('/');
            Ok(format!("{}/{}/{}", ep, self.bucket, key))
        }
    }

    pub async fn get_object(&self, key: &str) -> Result<(String, Vec<u8>), StorageError> {
        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| StorageError::Client(e.to_string()))?;

        let ct = resp
            .content_type()
            .unwrap_or_else(|| mime_guess(key))
            .to_string();
        let bytes = resp
            .body
            .collect()
            .await
            .map_err(|e| StorageError::Client(e.to_string()))?
            .into_bytes()
            .to_vec();
        Ok((ct, bytes))
    }

    pub async fn delete_object(&self, key: &str) -> Result<(), StorageError> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| StorageError::Client(e.to_string()))?;
        Ok(())
    }
}

#[derive(Clone)]
pub enum StorageBackend {
    Local(LocalStorage),
    Tigris(TigrisStorage),
}

impl StorageBackend {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Local(_) => "local",
            Self::Tigris(_) => "tigris",
        }
    }

    pub async fn put_object(
        &self,
        key: &str,
        content_type: &str,
        data: &[u8],
    ) -> Result<String, StorageError> {
        match self {
            Self::Local(s) => s.put_object(key, content_type, data).await,
            Self::Tigris(s) => s.put_object(key, content_type, data).await,
        }
    }

    pub async fn get_object(&self, key: &str) -> Result<(String, Vec<u8>), StorageError> {
        match self {
            Self::Local(s) => s.get_object(key).await,
            Self::Tigris(s) => s.get_object(key).await,
        }
    }

    pub async fn delete_object(&self, key: &str) -> Result<(), StorageError> {
        match self {
            Self::Local(s) => s.delete_object(key).await,
            Self::Tigris(s) => s.delete_object(key).await,
        }
    }
}

fn mime_guess(key: &str) -> &'static str {
    let lower = key.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".svg") {
        "image/svg+xml"
    } else if lower.ends_with(".mp4") {
        "video/mp4"
    } else if lower.ends_with(".webm") {
        "video/webm"
    } else if lower.ends_with(".mov") {
        "video/quicktime"
    } else if lower.ends_with(".mp3") {
        "audio/mpeg"
    } else if lower.ends_with(".ogg") {
        "audio/ogg"
    } else if lower.ends_with(".wav") {
        "audio/wav"
    } else {
        "application/octet-stream"
    }
}

pub fn init_storage(config: &Config) -> StorageBackend {
    if let (Some(bucket), Some(access_key), Some(secret_key)) = (
        config.s3_bucket.as_ref(),
        config.s3_access_key.as_ref(),
        config.s3_secret_key.as_ref(),
    ) {
        if !bucket.trim().is_empty() && !access_key.trim().is_empty() {
            return StorageBackend::Tigris(TigrisStorage::new(
                config.s3_endpoint.clone(),
                bucket.clone(),
                access_key.clone(),
                secret_key.clone(),
                config.s3_region.clone(),
                config.s3_public_url_prefix.clone(),
            ));
        }
    }

    StorageBackend::Local(LocalStorage::new(
        config.media_root.clone(),
        config.app_origin.clone(),
    ))
}
