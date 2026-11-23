//! One JSON line per message between a second launch and the running app.
use std::fmt;
use std::io::{self, BufRead, Read, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

/// Protocol version. Bump on any change an older build would misread.
pub const VERSION: u32 = 1;
/// A line longer than this is refused.
const MAX_LINE: u64 = 1 << 20;

/// What a second launch asks of the running app.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Request {
    /// Add these http(s) links (and show the window).
    Add { urls: Vec<String> },
    /// Bring the window to the front.
    Show,
}

/// The running app's answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reply {
    /// Carried out.
    Done,
    /// Refused, and why.
    Failed(String),
}

#[derive(Serialize, Deserialize)]
struct Envelope<T> {
    v: u32,
    body: T,
}

/// Why a line could not be read.
#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// The peer speaks another protocol version.
    Version(u32),
    Malformed,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Version(v) => write!(f, "protocol version {v}, expected {VERSION}"),
            Error::Malformed => write!(f, "malformed message"),
        }
    }
}

/// Writes `body` as one enveloped line.
pub fn send<T: Serialize>(out: &mut impl Write, body: &T) -> io::Result<()> {
    let mut line = serde_json::to_vec(&Envelope { v: VERSION, body }).map_err(io::Error::other)?;
    line.push(b'\n');
    out.write_all(&line)?;
    out.flush()
}

/// Reads one enveloped line. The body is decoded only when the version matches.
pub fn receive<T: DeserializeOwned>(input: &mut impl BufRead) -> Result<T, Error> {
    let mut line = Vec::new();
    let n = input
        .by_ref()
        .take(MAX_LINE)
        .read_until(b'\n', &mut line)
        .map_err(Error::Io)?;
    if n == 0 || line.last() != Some(&b'\n') {
        return Err(Error::Malformed);
    }
    let env: Envelope<Box<RawValue>> =
        serde_json::from_slice(&line).map_err(|_| Error::Malformed)?;
    if env.v != VERSION {
        return Err(Error::Version(env.v));
    }
    serde_json::from_str(env.body.get()).map_err(|_| Error::Malformed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_refusals() {
        let mut buf = Vec::new();
        send(
            &mut buf,
            &Request::Add {
                urls: vec!["https://a.b/".into()],
            },
        )
        .unwrap();
        assert!(buf.ends_with(b"\n"));
        let got: Request = receive(&mut buf.as_slice()).unwrap();
        assert_eq!(
            got,
            Request::Add {
                urls: vec!["https://a.b/".into()]
            }
        );

        let newer =
            br#"{"v":2,"body":{"nothing":1}}"#.iter().copied().chain(*b"\n").collect::<Vec<u8>>();
        assert!(matches!(
            receive::<Request>(&mut newer.as_slice()),
            Err(Error::Version(2))
        ));
        assert!(matches!(
            receive::<Request>(&mut &b"not json\n"[..]),
            Err(Error::Malformed)
        ));
        assert!(
            matches!(
                receive::<Request>(&mut &b"{\"v\":1,\"body\":\"show\"}"[..]),
                Err(Error::Malformed)
            ),
            "no newline"
        );
        assert!(matches!(
            receive::<Request>(&mut &b""[..]),
            Err(Error::Malformed)
        ));
        let long = vec![b'a'; (MAX_LINE + 10) as usize];
        assert!(receive::<Request>(&mut long.as_slice()).is_err());
    }
}
