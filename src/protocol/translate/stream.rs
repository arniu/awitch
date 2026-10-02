use axum::body::Bytes;
use futures_util::stream::{BoxStream, Stream, StreamExt};
use tokio::sync::oneshot;

use super::StreamTranslate;
use super::hub::Hub;
use crate::protocol::openai_chat::OpenaiChat;
use crate::protocol::{Metadata, StreamCodec};

const MAX_PEEK_BYTES: usize = 64 * 1024;

pub(crate) enum StreamEnd {
    Completed(Metadata),
    Errored,
    Dropped,
}

pub(crate) struct StreamOutcome {
    pub(crate) bytes: BoxStream<'static, Result<Bytes, std::io::Error>>,
    pub(crate) ended: oneshot::Receiver<StreamEnd>,
}

fn encode_sse_frame(event: Option<&str>, data: &str) -> String {
    match event {
        Some(et) => format!("event: {et}\ndata: {data}\n\n"),
        None => format!("data: {data}\n\n"),
    }
}

fn frame_lines(frame: &[u8]) -> impl Iterator<Item = &[u8]> + '_ {
    frame
        .split(|&b| b == b'\n')
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
}

fn frame_field<'a>(line: &'a [u8], field: &[u8]) -> Option<&'a [u8]> {
    let (name, value) = match line.iter().position(|&b| b == b':') {
        Some(i) => (&line[..i], &line[i + 1..]),
        None => (line, b"".as_slice()),
    };
    (name == field).then(|| value.strip_prefix(b" ").unwrap_or(value))
}

fn event_name(frame: &[u8]) -> Option<&[u8]> {
    frame_lines(frame)
        .filter_map(|line| frame_field(line, b"event"))
        .last()
}

fn data_payload(frame: &[u8]) -> Option<Vec<u8>> {
    let mut payload = Vec::new();
    let mut seen = false;

    for value in frame_lines(frame).filter_map(|line| frame_field(line, b"data")) {
        if seen {
            payload.push(b'\n');
        }
        seen = true;
        payload.extend_from_slice(value);
    }

    seen.then_some(payload)
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let mut start = 0;
    let mut end = bytes.len();
    while start < end && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    &bytes[start..end]
}

/// Buffer raw bytes into SSE frames, each yielded as it arrived, up to and
/// including its blank-line delimiter. Whatever the upstream closes on is the
/// last frame, delimiter or not.
fn parse_sse_frames<E: std::error::Error + Send + Sync + 'static>(
    body: impl Stream<Item = Result<Bytes, E>> + Send + Unpin + 'static,
) -> BoxStream<'static, Result<Bytes, E>> {
    futures_util::stream::unfold(
        (body, Vec::new(), Vec::<u8>::new()),
        |(mut body, mut buf, mut frame)| async move {
            loop {
                if let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                    let line: Vec<u8> = buf.drain(..=pos).collect();
                    if !trim_ascii(&line).is_empty() {
                        frame.extend_from_slice(&line);
                        continue;
                    }
                    if frame.is_empty() {
                        continue;
                    }
                    frame.extend_from_slice(&line);
                    let out = std::mem::take(&mut frame);
                    return Some((Ok(Bytes::from(out)), (body, buf, frame)));
                }
                match body.next().await {
                    Some(Ok(bytes)) => buf.extend_from_slice(&bytes),
                    Some(Err(e)) => {
                        return Some((Err(e), (body, buf, frame)));
                    }
                    None => {
                        frame.extend_from_slice(&buf);
                        buf.clear();
                        let out = std::mem::take(&mut frame);
                        return (!out.is_empty())
                            .then(|| (Ok(Bytes::from(out)), (body, buf, frame)));
                    }
                }
            }
        },
    )
    .boxed()
}

/// Whether the buffer holds a whole line carrying SSE data. The final segment
/// of a partial buffer is ignored: it may still be arriving.
fn has_data_line(buf: &[u8]) -> bool {
    let end = buf.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    frame_lines(&buf[..end]).any(|line| frame_field(line, b"data").is_some())
}

/// Drive a translated stream: decode upstream chat chunks into canonical hub
/// events, render them through the spoke, and re-encode for the client.
pub(crate) async fn translate_stream<S, X, E>(
    source: S,
    target: OpenaiChat,
    translate: X,
    raw_stream: impl Stream<Item = Result<Bytes, E>> + Send + Unpin + 'static,
) -> Result<StreamOutcome, E>
where
    S: StreamCodec,
    X: StreamTranslate<S>,
    E: std::error::Error + Send + Sync + 'static,
{
    let raw_stream = peek(raw_stream).await?;
    let (tx, ended) = oneshot::channel();
    let lifecycle = StreamLifecycle::new(tx);
    let bytes = build_stream(raw_stream, &source, &target, &translate, lifecycle)
        .map(Ok::<_, std::io::Error>)
        .boxed();
    Ok(StreamOutcome { bytes, ended })
}

/// Forward a same-protocol stream verbatim, reading only the metadata the
/// ledger needs from the source's own events.
pub(crate) async fn forward_stream<S, E>(
    source: S,
    raw_stream: impl Stream<Item = Result<Bytes, E>> + Send + Unpin + 'static,
) -> Result<StreamOutcome, E>
where
    S: StreamCodec,
    E: std::error::Error + Send + Sync + 'static,
{
    let raw_stream = peek(raw_stream).await?;
    let (tx, ended) = oneshot::channel();
    let lifecycle = StreamLifecycle::new(tx);
    let bytes = build_forward(raw_stream, &source, lifecycle)
        .map(Ok::<_, std::io::Error>)
        .boxed();
    Ok(StreamOutcome { bytes, ended })
}

async fn peek<E>(
    stream: impl Stream<Item = Result<Bytes, E>> + Send + Unpin + 'static,
) -> Result<BoxStream<'static, Result<Bytes, E>>, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    let mut stream = stream;
    let mut buf = Vec::new();

    loop {
        match stream.next().await {
            Some(Ok(bytes)) => {
                buf.extend_from_slice(&bytes);
                if has_data_line(&buf) || buf.len() >= MAX_PEEK_BYTES {
                    break;
                }
            }
            Some(Err(e)) => return Err(e),
            None => break,
        }
    }

    let raw = futures_util::stream::once(async move { Ok::<_, E>(Bytes::from(buf)) })
        .chain(stream)
        .boxed();

    Ok(raw)
}

fn sse_frame<S: StreamCodec>(source: &S, item: &S::Item) -> Bytes {
    let (event, data) = source.encode_item(item);
    Bytes::from(encode_sse_frame(event, &data))
}

fn sse_frames<S: StreamCodec>(source: &S, items: &[S::Item]) -> Vec<Bytes> {
    items.iter().map(|item| sse_frame(source, item)).collect()
}

struct StreamCtx<S: StreamCodec, X: StreamTranslate<S>> {
    source: S,
    target: OpenaiChat,
    translate: X,
    state: X::State,
    hub: Hub,
    meta: Metadata,
    lifecycle: StreamLifecycle,
}

impl<S: StreamCodec, X: StreamTranslate<S>> StreamCtx<S, X> {
    fn finalize(&mut self) -> Vec<S::Item> {
        let canonical = self.hub.finish();
        let mut events = Vec::new();
        for event in &canonical {
            events.extend(self.translate.transform_event(&mut self.state, event));
        }
        for ev in &events {
            self.source.observe(&mut self.meta, ev);
        }
        self.lifecycle.complete(self.meta.clone());
        events
    }

    fn process(&mut self, event: Option<&[u8]>, data: &[u8]) -> Option<Vec<S::Item>> {
        let event_str = event.and_then(|e| std::str::from_utf8(e).ok());
        let chunk = self.target.decode_item(event_str, data)?;
        let canonical = self.hub.feed(&chunk);
        let mut events = Vec::new();
        for event in &canonical {
            events.extend(self.translate.transform_event(&mut self.state, event));
        }
        for ev in &events {
            self.source.observe(&mut self.meta, ev);
        }
        Some(events)
    }

    fn errored(&mut self) {
        self.lifecycle.errored();
    }
}

struct ForwardCtx<S: StreamCodec> {
    source: S,
    meta: Metadata,
    lifecycle: StreamLifecycle,
}

impl<S: StreamCodec> ForwardCtx<S> {
    fn on_frame(&mut self, frame: &[u8]) -> bool {
        let Some(data) = data_payload(frame) else {
            return false;
        };
        if trim_ascii(&data) == b"[DONE]" {
            return true;
        }
        let event = event_name(frame).and_then(|e| std::str::from_utf8(e).ok());
        if let Some(item) = self.source.decode_item(event, &data) {
            self.source.observe(&mut self.meta, &item);
        }
        false
    }

    fn complete(&mut self) {
        self.lifecycle.complete(self.meta.clone());
    }

    fn errored(&mut self) {
        self.lifecycle.errored();
    }
}

struct StreamLifecycle {
    tx: Option<oneshot::Sender<StreamEnd>>,
}

impl StreamLifecycle {
    fn new(tx: oneshot::Sender<StreamEnd>) -> Self {
        StreamLifecycle { tx: Some(tx) }
    }

    fn complete(&mut self, meta: Metadata) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(StreamEnd::Completed(meta));
        }
    }

    fn errored(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(StreamEnd::Errored);
        }
    }
}

impl Drop for StreamLifecycle {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(StreamEnd::Dropped);
        }
    }
}

fn build_stream<S, X, E>(
    raw_stream: impl Stream<Item = Result<Bytes, E>> + Send + Unpin + 'static,
    source: &S,
    target: &OpenaiChat,
    translate: &X,
    lifecycle: StreamLifecycle,
) -> BoxStream<'static, Bytes>
where
    S: StreamCodec,
    X: StreamTranslate<S>,
    E: std::error::Error + Send + Sync + 'static,
{
    let ctx = StreamCtx {
        source: source.clone(),
        target: target.clone(),
        translate: translate.clone(),
        state: X::State::default(),
        hub: Hub::default(),
        meta: Metadata::default(),
        lifecycle,
    };

    let lines = parse_sse_frames(raw_stream.map(|r| r.map_err(std::io::Error::other)));

    futures_util::stream::unfold((lines, Some(ctx)), |(mut lines, mut opt_ctx)| async move {
        loop {
            opt_ctx.as_ref()?;
            match lines.next().await {
                Some(Ok(frame)) => {
                    let Some(data) = data_payload(&frame) else {
                        continue;
                    };
                    let data = trim_ascii(&data);
                    if data.is_empty() {
                        continue;
                    }
                    if data == b"[DONE]" {
                        let mut c = opt_ctx.take()?;
                        let events = c.finalize();
                        let out = sse_frames(&c.source, &events);
                        return (!out.is_empty()).then_some((out, (lines, Some(c))));
                    }

                    let c = opt_ctx.as_mut()?;
                    match c.process(event_name(&frame), data) {
                        Some(events) => {
                            let out = sse_frames(&c.source, &events);
                            if !out.is_empty() {
                                return Some((out, (lines, opt_ctx)));
                            }
                        }
                        None => {
                            tracing::warn!(
                                payload = %String::from_utf8_lossy(&data[..data.len().min(120)]),
                                "upstream frame is not a hub chunk; ending the stream"
                            );
                            c.errored();
                            return None;
                        }
                    }
                }
                Some(Err(e)) => {
                    tracing::warn!(error = %e, "upstream stream errored mid-stream");
                    if let Some(mut c) = opt_ctx.take() {
                        c.errored();
                    }
                    return None;
                }
                None => {
                    let mut c = opt_ctx.take()?;
                    let events = c.finalize();
                    let out = sse_frames(&c.source, &events);
                    return (!out.is_empty()).then_some((out, (lines, None)));
                }
            }
        }
    })
    .flat_map(futures_util::stream::iter)
    .boxed()
}

fn build_forward<S, E>(
    raw_stream: impl Stream<Item = Result<Bytes, E>> + Send + Unpin + 'static,
    source: &S,
    lifecycle: StreamLifecycle,
) -> BoxStream<'static, Bytes>
where
    S: StreamCodec,
    E: std::error::Error + Send + Sync + 'static,
{
    let ctx = ForwardCtx {
        source: source.clone(),
        meta: Metadata::default(),
        lifecycle,
    };

    let lines = parse_sse_frames(raw_stream.map(|r| r.map_err(std::io::Error::other)));

    futures_util::stream::unfold((lines, Some(ctx)), |(mut lines, mut opt_ctx)| async move {
        opt_ctx.as_ref()?;
        match lines.next().await {
            Some(Ok(frame)) => {
                let done = opt_ctx.as_mut()?.on_frame(&frame);
                if done {
                    let mut c = opt_ctx.take()?;
                    c.complete();
                    Some((vec![frame], (lines, None)))
                } else {
                    Some((vec![frame], (lines, opt_ctx)))
                }
            }
            Some(Err(e)) => {
                tracing::warn!(error = %e, "upstream stream errored mid-stream");
                if let Some(mut c) = opt_ctx.take() {
                    c.errored();
                }
                None
            }
            None => {
                let mut c = opt_ctx.take()?;
                c.complete();
                None
            }
        }
    })
    .flat_map(futures_util::stream::iter)
    .boxed()
}
