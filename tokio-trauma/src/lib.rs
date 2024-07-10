//! Asynchronous downloader based on trauma recurrence
//!
//! # Examples
//!
//! basic usage
//!
//! ```rust
//! use std::path::PathBuf;
//!
//! use url::Url;
//!
//! use tokio_trauma::download::Download;
//! use tokio_trauma::downloader::DownloaderBuilder;
//!
//! #[tokio::main]
//! async fn main() {
//!     let url = "http://localhost:9876/test/file";
//!     let download = Download::new(Url::parse(url).unwrap(), "algorithm-rs-master.zip".into());
//!     let downloader = DownloaderBuilder::new()
//!         .directory(PathBuf::from("E:\\data"))
//!         .build();
//!     let summary = downloader.download(&vec![download]).await;
//! }
//! ```

#![feature(core_intrinsics)]

pub mod download;
pub mod error;
pub mod downloader;