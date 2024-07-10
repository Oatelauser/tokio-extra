use std::{env, fs, io};
use std::path::PathBuf;

use futures_util::{stream, StreamExt};
use reqwest::{Proxy, StatusCode};
use reqwest::header::{HeaderMap, HeaderValue, IntoHeaderName, RANGE};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::RetryTransientMiddleware;
use reqwest_tracing::{DefaultSpanBackend, TracingMiddleware};
use retry_policies::policies::ExponentialBackoff;
use snafu::{location, Location, ResultExt};
use tokio::fs::OpenOptions;
use tokio::io::{AsyncWriteExt, BufWriter};
use url::Url;

use crate::download::{Download, Status, Summary};
use crate::error::{ReqwestSnafu, Result};

#[derive(Debug, Clone)]
pub struct Downloader {
    directory: PathBuf,
    retries: u32,
    concurrent_downloads: u8,
    resume: bool,
    headers: Option<HeaderMap>,
}

impl Downloader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn builder(self) -> DownloaderBuilder {
        DownloaderBuilder(self)
    }
}

impl Downloader {
    pub async fn download(&self, downloads: impl AsRef<[Download]>) -> Result<Vec<Summary>> {
        self.proxy_download(downloads.as_ref(), None).await
    }

    pub async fn proxy_download(&self, downloads: &[Download], proxy: Option<Proxy>) -> Result<Vec<Summary>> {
        let mut client_builder = reqwest::Client::builder();
        if let Some(proxy) = proxy {
            client_builder = client_builder.proxy(proxy);
        }
        if let Some(headers) = &self.headers {
            client_builder = client_builder.default_headers(headers.clone());
        }
        let client = client_builder.build()
            .context(ReqwestSnafu { location: location!() })?;

        let retry_policy = ExponentialBackoff::builder()
            .build_with_max_retries(self.retries);
        let client = ClientBuilder::new(client)
            .with(TracingMiddleware::<DefaultSpanBackend>::new())  // Trace Http Request
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))  // Retry failed requests
            .build();

        let summaries = stream::iter(downloads)
            .map(|download| self.fetch(&client, download))
            .buffer_unordered(self.concurrent_downloads as usize)
            .collect()
            .await;
        Ok(summaries)
    }

    async fn fetch(&self, client: &ClientWithMiddleware, download: &Download) -> Summary {
        let mut size_on_disk: u64 = 0;
        let mut can_resume = false;
        let output_path = self.directory.join(&download.filename);
        let mut summary = Summary {
            download: download.clone(),
            status_code: StatusCode::BAD_REQUEST,
            size: size_on_disk,
            status: Status::NotStarted,
            resume: can_resume,
        };
        let mut content_length = None;

        // Handling interrupted file downloads
        if self.resume {
            match download.fetch_range(client).await {
                Ok(data) => {
                    can_resume = data.resume;
                    content_length = data.size;
                }
                Err(err) => return summary.fail(err),
            };

            // check if there is a file on disk already
            if can_resume && output_path.exists() {
                size_on_disk = match output_path.metadata() {
                    Ok(metadata) => metadata.len(),
                    Err(err) => return summary.fail(err),
                };
            }

            // update summary resume field
            summary.resume = can_resume;
        }

        // 1.If content_length exists and is equal to the size of the file, the download is considered complete.
        // 2.If the file size is not empty and is equal to the sum of the two, it is considered that the download is completed.
        let size = content_length.unwrap_or_default() + size_on_disk;
        if matches!(content_length, Some(content_length) if content_length == size_on_disk) ||
            size_on_disk > 0 && size == size_on_disk {
            return summary.with_status(Status::Skipped(String::from("the file was already full download")));
        }

        // Create download request object
        tracing::debug!("Fetching Url: {}", &download.url);
        let mut request = client.get(download.url.as_str());
        if self.resume && can_resume {
            request = request.header(RANGE, format!("bytes={}-", size_on_disk));
        }
        if let Some(ref header) = self.headers {
            request = request.headers(header.clone());
        }

        // Sending download request
        let response = match request.send().await {
            Ok(response) => response,
            Err(err) => return summary.fail(err),
        };
        summary.status_code = response.status();
        summary.size = size;
        summary.resume = can_resume;
        if let Err(err) = response.error_for_status_ref() {
            return summary.fail(err);
        }

        // Process the directory where downloaded files are stored
        let folder = output_path.parent().unwrap_or(&output_path);
        tracing::debug!("Creating destination directory {:?}", folder);
        if let Err(err) = fs::create_dir_all(folder) {
            return summary.fail(err);
        }

        let result = OpenOptions::new().create(true)
            .write(true).append(can_resume)
            .open(output_path).await;
        let file = match result {
            Ok(file) => file,
            Err(err) => return summary.fail(err),
        };
        let mut file = BufWriter::new(file);

        // Stream response content and write to file
        let mut final_size = size_on_disk;
        let mut stream = response.bytes_stream();
        while let Some(data) = stream.next().await {
            let mut chunk = match data {
                Ok(chunk) => chunk,
                Err(err) => return summary.fail(err),
            };

            final_size += chunk.len() as u64;
            match file.write_all_buf(&mut chunk).await {
                Ok(_) => {}
                Err(err) => return summary.fail(err),
            }
        }

        summary.with_status(Status::Success)
    }
}

impl Default for Downloader {
    fn default() -> Self {
        Self {
            directory: env::current_dir().unwrap_or_default(),
            retries: 0,
            concurrent_downloads: 32,
            resume: true,
            headers: None,
        }
    }
}

#[repr(transparent)]
#[derive(Debug, Clone)]
pub struct DownloaderBuilder(Downloader);

impl DownloaderBuilder {
    pub fn new() -> Self {
        Self(Downloader::new())
    }

    pub fn directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.0.directory = directory.into();
        self
    }

    pub fn retries(mut self, retries: u32) -> Self {
        self.0.retries = retries;
        self
    }

    pub fn concurrent_downloads(mut self, concurrent: u8) -> Self {
        self.0.concurrent_downloads = concurrent;
        self
    }

    pub fn headers(mut self, headers: HeaderMap) -> Self {
        let headers = match self.0.headers {
            None => HeaderMap::from(headers),
            Some(mut header) => {
                header.extend(headers);
                header
            }
        };
        self.0.headers = Some(headers);
        self
    }

    pub fn header<K: IntoHeaderName>(mut self, name: K, value: HeaderValue) -> Self {
        let headers = match self.0.headers {
            None => {
                let mut headers = HeaderMap::new();
                headers.insert(name, value);
                headers
            }
            Some(mut headers) => {
                headers.insert(name, value);
                headers
            }
        };
        self.0.headers = Some(headers);
        self
    }

    pub fn build(self) -> Downloader {
        self.0
    }
}
