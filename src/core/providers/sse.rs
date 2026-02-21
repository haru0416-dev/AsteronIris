#[derive(Debug, Default)]
pub struct SseBuffer {
    buffer: String,
}

impl SseBuffer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
        }
    }

    pub fn push_chunk(&mut self, chunk: &[u8]) {
        let text = String::from_utf8_lossy(chunk);
        self.buffer.push_str(&text);
    }

    pub fn next_event_block(&mut self) -> Option<String> {
        let boundary = self.buffer.find("\n\n")?;
        let remaining = self.buffer.split_off(boundary + 2);
        let event_block = std::mem::take(&mut self.buffer);
        self.buffer = remaining;
        Some(event_block)
    }
}

pub fn parse_data_lines(event_block: &str) -> Vec<&str> {
    event_block
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .collect()
}

pub fn parse_data_lines_without_done(event_block: &str) -> Vec<&str> {
    parse_data_lines(event_block)
        .into_iter()
        .filter(|data| *data != "[DONE]")
        .collect()
}

pub fn parse_event_data_pairs(event_block: &str) -> Vec<(&str, &str)> {
    let mut events = Vec::new();
    let mut current_event = None;

    for line in event_block.lines() {
        if let Some(event_type) = line.strip_prefix("event: ") {
            current_event = Some(event_type.trim());
        } else if let Some(data) = line.strip_prefix("data: ")
            && let Some(event_type) = current_event.take()
        {
            events.push((event_type, data.trim()));
        }
    }

    events
}

#[cfg(test)]
mod tests {
    use super::{
        SseBuffer, parse_data_lines, parse_data_lines_without_done, parse_event_data_pairs,
    };

    #[test]
    fn next_event_block_returns_complete_frames_only() {
        let mut buffer = SseBuffer::new();
        buffer.push_chunk(b"data: first\n\npartial");

        assert_eq!(
            buffer.next_event_block().as_deref(),
            Some("data: first\n\n")
        );
        assert!(buffer.next_event_block().is_none());

        buffer.push_chunk(b"ly\n\n");
        assert_eq!(buffer.next_event_block().as_deref(), Some("partially\n\n"));
    }

    #[test]
    fn parse_data_lines_extracts_data_prefix_lines() {
        let block = "event: message\ndata: one\nfoo: ignored\ndata: two\n\n";
        assert_eq!(parse_data_lines(block), vec!["one", "two"]);
    }

    #[test]
    fn parse_data_lines_without_done_filters_sentinel() {
        let block = "data: [DONE]\ndata: payload\n\n";
        assert_eq!(parse_data_lines_without_done(block), vec!["payload"]);
    }

    #[test]
    fn parse_event_data_pairs_matches_event_to_next_data() {
        let block = concat!(
            "event: message_start\n",
            "data: {\"message\":{}}\n",
            "data: ignored\n",
            "event: content_block_delta\n",
            "data: {\"delta\":{}}\n\n"
        );

        assert_eq!(
            parse_event_data_pairs(block),
            vec![
                ("message_start", "{\"message\":{}}"),
                ("content_block_delta", "{\"delta\":{}}")
            ]
        );
    }
}
