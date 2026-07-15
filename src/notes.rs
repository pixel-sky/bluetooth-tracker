use crate::{
    address::BluetoothAddress,
    paths::TrackerPaths,
    storage::{NoteOutcome, SpanBoundary, set_span_note},
};
use anyhow::Result;

pub fn add_note(
    paths: &TrackerPaths,
    address: Option<&BluetoothAddress>,
    boundary: SpanBoundary,
    text: impl AsRef<[String]>,
) -> Result<()> {
    let outcome = set_span_note(paths, address, boundary, text.as_ref().join(" "))?;
    match outcome {
        NoteOutcome::ActiveSpan(address) => println!(
            "Added {} note to active span for {}",
            boundary.label(),
            address
        ),
        NoteOutcome::LatestSpan(address) => println!(
            "Added {} note to latest span for {}",
            boundary.label(),
            address
        ),
    }
    Ok(())
}
