use std::string::FromUtf8Error;

use snafu::{Location, Snafu};
use snafu_stack_error::stack_trace_debug;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Snafu)]
#[snafu(visibility(pub))]
#[stack_trace_debug]
pub enum Error {
    /// invalid url error
    #[snafu(display("Invalid url: {}", message))]
    InvalidUrl {
        message: String,
        location: Location,
    },

    /// encode url error
    #[snafu(display("Failed encode Url: {}", url))]
    EncodeUrl {
        url: String,
        location: Location,
        #[snafu(source)]
        error: FromUtf8Error,
    },

    #[snafu(display("Parse url error: {}", url))]
    ParseUrl {
        url: String,
        location: Location,
        #[snafu(source)]
        error: url::ParseError,
    },

    #[snafu(display("Call reqwest failed"))]
    Reqwest {
        location: Location,
        #[snafu(source)]
        error: reqwest::Error,
    },
}
