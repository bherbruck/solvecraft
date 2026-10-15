//! A Server-Sent Events decoder that takes bytes as they arrive from the network.
//!
//! Chunks may end anywhere: inside a line, inside a UTF-8 sequence, between the `\r` and `\n` of
//! a line break. Lines are split on bytes and only complete lines are decoded as text. The
//! server is untrusted, so a single event and the pending line have size caps.

/// Largest event (all its `data:` lines) accepted.
pub const MAX_EVENT: usize = 1 << 20;

/// One dispatched event.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SseEvent {
    /// The `event:` field, if the server sent one.
    pub name: Option<String>,
    /// The `data:` lines joined with `\n`.
    pub data: String,
}

#[derive(Debug, Default)]
pub struct SseDecoder {
    line: Vec<u8>,
    event: SseEvent,
    has_data: bool,
    /// The previous byte was `\r`: a following `\n` belongs to the same line break.
    after_cr: bool,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a chunk; complete events are appended to `out`. Fails when an event grows past
    /// [`MAX_EVENT`].
    pub fn push(&mut self, bytes: &[u8], out: &mut Vec<SseEvent>) -> Result<(), String> {
        for &b in bytes {
            if self.after_cr {
                self.after_cr = false;
                if b == b'\n' {
                    continue;
                }
            }
            match b {
                b'\r' => {
                    self.after_cr = true;
                    self.end_line(out)?;
                }
                b'\n' => self.end_line(out)?,
                _ => {
                    if self.line.len() >= MAX_EVENT {
                        return Err("the server sent a line that is too long".into());
                    }
                    self.line.push(b);
                }
            }
        }
        Ok(())
    }

    /// The stream ended: an event without its closing blank line is still delivered.
    pub fn finish(&mut self, out: &mut Vec<SseEvent>) -> Result<(), String> {
        if !self.line.is_empty() {
            self.end_line(out)?;
        }
        self.dispatch(out);
        Ok(())
    }

    fn end_line(&mut self, out: &mut Vec<SseEvent>) -> Result<(), String> {
        let line = String::from_utf8_lossy(&std::mem::take(&mut self.line)).into_owned();
        if line.is_empty() {
            self.dispatch(out);
            return Ok(());
        }
        if line.starts_with(':') {
            return Ok(());
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line.as_str(), ""),
        };
        match field {
            "data" => {
                if self.event.data.len() + value.len() + 1 > MAX_EVENT {
                    return Err("the server sent an event that is too large".into());
                }
                if self.has_data {
                    self.event.data.push('\n');
                }
                self.event.data.push_str(value);
                self.has_data = true;
            }
            "event" => self.event.name = Some(value.to_string()),
            _ => {}
        }
        Ok(())
    }

    fn dispatch(&mut self, out: &mut Vec<SseEvent>) {
        let ev = std::mem::take(&mut self.event);
        if std::mem::take(&mut self.has_data) {
            out.push(ev);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(chunks: &[&[u8]]) -> Vec<SseEvent> {
        let mut d = SseDecoder::new();
        let mut out = Vec::new();
        for c in chunks {
            d.push(c, &mut out).unwrap();
        }
        d.finish(&mut out).unwrap();
        out
    }

    fn ev(name: Option<&str>, data: &str) -> SseEvent {
        SseEvent { name: name.map(str::to_string), data: data.to_string() }
    }

    #[test]
    fn events_lines_and_comments() {
        let out = decode(&[b": ping\n\nevent: a\ndata: one\n\ndata: x\ndata: y\n\n"]);
        assert_eq!(out, vec![ev(Some("a"), "one"), ev(None, "x\ny")]);
    }

    #[test]
    fn line_breaks_in_every_flavour() {
        let want = vec![ev(None, "a"), ev(None, "b"), ev(None, "c")];
        assert_eq!(decode(&[b"data: a\n\ndata: b\n\ndata: c\n\n"]), want);
        assert_eq!(decode(&[b"data: a\r\n\r\ndata: b\r\n\r\ndata: c\r\n\r\n"]), want);
        assert_eq!(decode(&[b"data: a\r\rdata: b\r\rdata: c\r\r"]), want);
    }

    /// Splitting the stream at every byte position gives the same events, also inside a
    /// multi-byte character and between `\r` and `\n`.
    #[test]
    fn any_chunk_boundary_gives_the_same_events() {
        let stream = "event: x\r\ndata: grüße 🌍 {\"a\":1}\r\n\r\n: c\r\ndata: [DONE]\n\n".as_bytes();
        let whole = decode(&[stream]);
        assert_eq!(whole.len(), 2);
        assert_eq!(whole[0].data, "grüße 🌍 {\"a\":1}");
        for i in 0..=stream.len() {
            assert_eq!(decode(&[&stream[..i], &stream[i..]]), whole, "split at {i}");
        }
        let singles: Vec<&[u8]> = stream.chunks(1).collect();
        assert_eq!(decode(&singles), whole);
    }

    #[test]
    fn an_unterminated_last_event_is_delivered() {
        assert_eq!(decode(&[b"data: tail"]), vec![ev(None, "tail")]);
        assert!(decode(&[b"event: only"]).is_empty());
    }

    #[test]
    fn oversized_input_is_an_error_not_memory() {
        let mut d = SseDecoder::new();
        let mut out = Vec::new();
        let big = vec![b'a'; MAX_EVENT + 10];
        assert!(d.push(&big, &mut out).is_err());
        let mut d = SseDecoder::new();
        let line = format!("data: {}\n", "b".repeat(MAX_EVENT / 2 + 1));
        let mut failed = false;
        for _ in 0..3 {
            if d.push(line.as_bytes(), &mut out).is_err() {
                failed = true;
                break;
            }
        }
        assert!(failed);
    }

    #[test]
    fn invalid_utf8_is_replaced_not_fatal() {
        let out = decode(&[b"data: a\xffb\n\n"]);
        assert_eq!(out.len(), 1);
        assert!(out[0].data.starts_with('a') && out[0].data.ends_with('b'));
    }
}
