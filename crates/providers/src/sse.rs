//! Incremental Server-Sent Events parser.

/// Accumulates bytes and yields complete `data:` payloads.
#[derive(Debug, Default)]
pub struct SseParser {
    buffer: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseEvent {
    Data(String),
    Done,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a chunk and return every complete event it completes.
    pub fn feed(&mut self, chunk: &str) -> Vec<SseEvent> {
        self.buffer.push_str(chunk);
        let mut events = Vec::new();
        loop {
            // An event ends at a blank line (\n\n or \r\n\r\n).
            let Some(idx) = find_event_end(&self.buffer) else { break };
            let (raw, rest_start) = idx;
            let block = self.buffer[..raw].to_string();
            self.buffer.drain(..rest_start);
            if let Some(ev) = parse_block(&block) {
                events.push(ev);
            }
        }
        events
    }

    /// Flush any trailing partial event (stream ended without a blank line).
    pub fn finish(&mut self) -> Option<SseEvent> {
        let block = std::mem::take(&mut self.buffer);
        if block.trim().is_empty() {
            None
        } else {
            parse_block(&block)
        }
    }
}

fn find_event_end(buf: &str) -> Option<(usize, usize)> {
    let lf = buf.find("\n\n").map(|i| (i, i + 2));
    let crlf = buf.find("\r\n\r\n").map(|i| (i, i + 4));
    match (lf, crlf) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn parse_block(block: &str) -> Option<SseEvent> {
    let mut data_lines = Vec::new();
    for line in block.lines() {
        let line = line.trim_end_matches('\r');
        if line.starts_with(':') || line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.strip_prefix(' ').unwrap_or(rest));
        }
        // `event:`, `id:`, `retry:` fields are ignored for chat completions.
    }
    if data_lines.is_empty() {
        return None;
    }
    let data = data_lines.join("\n");
    if data.trim() == "[DONE]" {
        Some(SseEvent::Done)
    } else {
        Some(SseEvent::Data(data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_split_chunks() {
        let mut p = SseParser::new();
        assert!(p.feed("data: {\"a\":").is_empty());
        let evs = p.feed("1}\n\ndata: [DONE]\n\n");
        assert_eq!(evs, vec![SseEvent::Data("{\"a\":1}".into()), SseEvent::Done]);
    }

    #[test]
    fn handles_crlf_and_comments() {
        let mut p = SseParser::new();
        let evs = p.feed(": keepalive\r\ndata: x\r\n\r\n");
        assert_eq!(evs, vec![SseEvent::Data("x".into())]);
    }

    #[test]
    fn finish_flushes_partial() {
        let mut p = SseParser::new();
        assert!(p.feed("data: tail").is_empty());
        assert_eq!(p.finish(), Some(SseEvent::Data("tail".into())));
    }
}
