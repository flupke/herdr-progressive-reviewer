//! What the page tells the reviewer after a post it could not carry out. The post redirects to
//! the page, so the notice travels in a cookie that the next load shows once and clears.

use axum::http::{HeaderMap, HeaderValue};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Serialize;

use crate::access::cookie;
use crate::command::CommandRefusal;

/// The longest reason a notice keeps, in bytes, so that its cookie stays small.
const REASON_LIMIT: usize = 1000;

/// Why a post did not go through, as the template tests it.
#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "reason", rename_all = "snake_case")]
pub(crate) enum Notice {
    /// The round moved on since the page was loaded.
    Stale,
    /// The round's owner could not carry out the post, for this reason.
    Failed(String),
    /// The round's owner did not reply: the post may have gone through.
    NoReply,
}

impl From<CommandRefusal> for Notice {
    fn from(refusal: CommandRefusal) -> Self {
        match refusal {
            CommandRefusal::Stale => Self::Stale,
            CommandRefusal::Failed(reason) => Self::Failed(reason),
        }
    }
}

impl Notice {
    const COOKIE: &str = "explore_notice";

    /// The notice a request's cookie carries.
    pub(crate) fn read(headers: &HeaderMap) -> Option<Self> {
        let value = cookie(headers, Self::COOKIE)?;
        match value {
            "stale" => Some(Self::Stale),
            "no-reply" => Some(Self::NoReply),
            _ => {
                let encoded = value.strip_prefix("failed.")?;
                let reason = URL_SAFE_NO_PAD.decode(encoded).ok()?;
                Some(Self::Failed(String::from_utf8(reason).ok()?))
            }
        }
    }

    /// The `Set-Cookie` value that carries the notice to the next load.
    pub(crate) fn cookie(&self) -> HeaderValue {
        let value = match self {
            Self::Stale => "stale".to_owned(),
            Self::NoReply => "no-reply".to_owned(),
            Self::Failed(reason) => {
                let mut end = reason.len().min(REASON_LIMIT);
                while !reason.is_char_boundary(end) {
                    end -= 1;
                }
                format!("failed.{}", URL_SAFE_NO_PAD.encode(&reason[..end]))
            }
        };
        // The post redirects at once, so the next load comes within seconds.
        Self::header(&value, 10)
    }

    /// The `Set-Cookie` value that clears the notice once shown.
    pub(crate) fn clear() -> HeaderValue {
        Self::header("", 0)
    }

    fn header(value: &str, max_age: u32) -> HeaderValue {
        // The value holds only letters, digits, `-`, `_` and `.`.
        HeaderValue::from_str(&format!(
            "{}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}",
            Self::COOKIE
        ))
        .expect("a notice cookie is a valid header")
    }
}
