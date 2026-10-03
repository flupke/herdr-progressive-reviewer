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

/// A post that did not go through, and why, as the template `notice.html` reads it: it shows
/// the partial `notice-{post}.html`, which words the notice for that post.
#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct Notice {
    post: Post,
    problem: Problem,
}

/// What the reviewer asked for with the post. It serializes as its [name](Self::name).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Post {
    /// An answer to the question the page showed.
    Answer,
    /// The start of a round.
    Start,
    /// The reviewer's first pick of a blind question, before the recommendation shows.
    Pick,
}

/// Why a post did not go through, as the templates test it.
#[derive(Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "reason", rename_all = "snake_case")]
pub(crate) enum Problem {
    /// The round moved on since the page was loaded.
    Stale,
    /// The round's owner could not carry out the post, for this reason.
    Failed(String),
    /// The round's owner did not reply: the post may have gone through.
    NoReply,
}

impl From<CommandRefusal> for Problem {
    fn from(refusal: CommandRefusal) -> Self {
        match refusal {
            CommandRefusal::Stale => Self::Stale,
            CommandRefusal::Failed(reason) => Self::Failed(reason),
        }
    }
}

impl Post {
    const ALL: [Self; 3] = [Self::Answer, Self::Start, Self::Pick];

    /// The post's name in the cookie, as in the name of its template partial.
    fn name(self) -> &'static str {
        match self {
            Self::Answer => "answer",
            Self::Start => "start",
            Self::Pick => "pick",
        }
    }
}

impl Serialize for Post {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl Notice {
    const COOKIE: &str = "explore_notice";

    pub(crate) fn new(post: Post, problem: Problem) -> Self {
        Self { post, problem }
    }

    /// The notice a request's cookie carries.
    pub(crate) fn read(headers: &HeaderMap) -> Option<Self> {
        let (post, problem) = cookie(headers, Self::COOKIE)?.split_once('.')?;
        let post = Post::ALL.into_iter().find(|known| known.name() == post)?;
        let problem = match problem {
            "stale" => Problem::Stale,
            "no-reply" => Problem::NoReply,
            _ => {
                let encoded = problem.strip_prefix("failed.")?;
                let reason = URL_SAFE_NO_PAD.decode(encoded).ok()?;
                Problem::Failed(String::from_utf8(reason).ok()?)
            }
        };
        Some(Self { post, problem })
    }

    /// The `Set-Cookie` value that carries the notice to the next load.
    pub(crate) fn cookie(&self) -> HeaderValue {
        let problem = match &self.problem {
            Problem::Stale => "stale".to_owned(),
            Problem::NoReply => "no-reply".to_owned(),
            Problem::Failed(reason) => {
                let mut end = reason.len().min(REASON_LIMIT);
                while !reason.is_char_boundary(end) {
                    end -= 1;
                }
                format!("failed.{}", URL_SAFE_NO_PAD.encode(&reason[..end]))
            }
        };
        // The post redirects at once, so the next load comes within seconds.
        Self::header(&format!("{}.{problem}", self.post.name()), 10)
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
