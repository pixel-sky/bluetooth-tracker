use crate::{
    paths::TrackerPaths,
    storage::{set_span_note, NoteOutcome, SpanBoundary},
};
use anyhow::Result;

pub fn add_note(paths: &TrackerPaths, boundary: SpanBoundary, text: &[String]) -> Result<()> {
    let outcome = set_span_note(paths, boundary, &text.join(" "))?;
    println!(
        "Added {} note to {}",
        boundary.label(),
        match outcome {
            NoteOutcome::ActiveSpan => "active span",
            NoteOutcome::LatestSpan => "latest span",
        }
    );
    Ok(())
}
