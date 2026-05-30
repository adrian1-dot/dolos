use dolos_core::config::RootConfig;
use dolos_core::ImportExt;
use itertools::Itertools;
use miette::{Context, IntoDiagnostic};

use dolos::prelude::*;

use crate::feedback::Feedback;

#[derive(Debug, clap::Args)]
pub struct Args {
    #[arg(short, long, default_value_t = 500)]
    pub chunk: usize,
}

pub fn run(config: &RootConfig, args: &Args, feedback: &Feedback) -> miette::Result<()> {
    //crate::common::setup_tracing(&config.logging)?;

    let progress = feedback.slot_progress_bar();
    progress.set_message("rebuilding stores");

    let domain = crate::common::setup_domain(config)?;

    let (tip, _) = domain
        .wal
        .find_tip()
        .into_diagnostic()
        .context("finding WAL tip")?
        .ok_or(miette::miette!("no WAL tip found"))?;

    progress.set_length(tip.slot());

    let remaining = domain
        .wal
        .iter_blocks(None, None)
        .into_diagnostic()
        .context("iterating over wal blocks")?
        // skip empty WAL entries (origin marker) — these can't be decoded/imported
        .filter(|(point, body)| {
            if body.is_empty() {
                tracing::warn!(slot = point.slot(), "skipping empty WAL block");
                false
            } else {
                true
            }
        });

    for chunk in remaining.chunks(args.chunk).into_iter() {
        // collect the chunk into a vector so we can report slot range on error
        let chunk_vec: Vec<_> = chunk.into_iter().collect();

        let first_slot = chunk_vec.first().map(|(p, _)| p.slot());
        let last_slot = chunk_vec.last().map(|(p, _)| p.slot());

        let collected = chunk_vec.iter().map(|(_, x)| x.clone()).collect_vec();

        match domain.import_blocks(collected) {
            Ok(cursor) => progress.set_position(cursor),
            Err(e) => {
                let start = first_slot.map(|s: u64| s.to_string()).unwrap_or_else(|| "?".into());
                let end = last_slot.map(|s: u64| s.to_string()).unwrap_or_else(|| "?".into());

                miette::bail!(format!(
                    "failed to apply block chunk (slots {start}-{end}): {e}"
                ));
            }
        }
    }

    Ok(())
}
