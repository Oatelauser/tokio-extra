use std::fmt::Display;
use reqwest::{StatusCode, Url};
use reqwest::header::{ACCEPT_RANGES, CONTENT_LENGTH};
use reqwest_middleware::{ClientWithMiddleware, Result as ReqResult};
use snafu::{location, Location, OptionExt, ResultExt};

use crate::error::{EncodeUrlSnafu, InvalidUrlSnafu, ParseUrlSnafu};

#[derive(Debug, Clone)]
pub struct Download {
    pub url: Url,
    pub filename: String,
}

impl Download {
    pub fn new(url: Url, filename: String) -> Self {
        Self { url, filename }
    }

    /// Send http head method range request
    ///
    /// Determine whether the service supports range requests and the size of the resource
    ///
    /// # Examples
    ///
    /// basic usage:
    ///
    /// ```
    /// use reqwest_middleware::ClientWithMiddleware;
    /// use trauma::download::Download;
    ///
    /// let download = Download::try_from("https://github.com/seanmonstar/reqwest/archive/refs/tags/v0.11.9.zip").unwrap();
    /// let  client = ClientWithMiddleware::from(reqwest::Client::builder().build().unwrap());
    /// let  content_range = download.fetch_range(&client);
    /// ```
    pub async fn fetch_range(&self, client: &ClientWithMiddleware) -> ReqResult<ContentRange> {
        let response = client.head(self.url.as_str()).send().await?;
        let headers = response.headers();

        let resume = match headers.get(ACCEPT_RANGES) {
            Some(val) if val == "none" => false,
            Some(_) => true,
            None => false,
        };
        let size = headers.get(CONTENT_LENGTH)
            .and_then(|val| val.to_str().ok())
            .and_then(|val| val.parse().ok());

        Ok(ContentRange { resume, size })
    }
}

impl TryFrom<&Url> for Download {
    type Error = crate::error::Error;

    fn try_from(url: &Url) -> Result<Self, Self::Error> {
        let segment = url.path_segments().with_context(|| {
            let message = format!("the url [{}] does not contain a valid path", url);
            InvalidUrlSnafu { message, location: location!() }
        })?.last().with_context(|| {
            let message = format!("the url [{}] does not contain a filename", url);
            InvalidUrlSnafu { message, location: location!() }
        })?;

        let filename = urlencoding::decode(segment)
            .context(EncodeUrlSnafu { url: url.as_str(), location: location!() })?
            .to_string();
        Ok(Download {
            url: url.clone(),
            filename,
        })
    }
}

impl TryFrom<&str> for Download {
    type Error = crate::error::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let url = Url::parse(value)
            .context(ParseUrlSnafu { url: value, location: location!() })?;
        Self::try_from(&url)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ContentRange {
    pub resume: bool,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Status {
    Fail(String),
    NotStarted,
    Skipped(String),
    Success,
}

#[derive(Debug, Clone)]
pub struct Summary {
    pub(crate) download: Download,
    /// http response status code
    pub(crate) status_code: StatusCode,
    /// download size in bytes
    pub(crate) size: u64,
    pub(crate) status: Status,
    pub(crate) resume: bool,
}

impl Summary {
    pub fn with_status(self, status: Status) -> Self {
        Self { status, ..self }
    }

    pub fn fail(self, msg: impl Display) -> Self {
        Self { status: Status::Fail(msg.to_string()), ..self }
    }

    pub fn resumable(&mut self, resume: bool) {
        self.resume = resume
    }

    pub fn download(&self) -> &Download {
        &self.download
    }

    pub fn status_code(&self) -> &StatusCode {
        &self.status_code
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    pub fn resume(&self) -> bool {
        self.resume
    }
}

#[cfg(test)]
mod test {
    use url::Url;

    use crate::download::Download;

    const DOMAIN: &str = "http://domain.com/file.zip";

    #[test]
    fn test_url() {
        let url = Url::parse(DOMAIN).unwrap();
        let download = Download::try_from(&url).unwrap();
        assert_eq!("file.zip", download.filename)
    }

    #[test]
    fn test_string() {
        let download = Download::try_from(DOMAIN).unwrap();
        assert_eq!("file.zip", download.filename)
    }
}
