use dolos_core::{BlockBody, BlockHash, BlockSlot, EraCbor, RawBlock, RawUtxoMap, TxoRef};
use pallas::ledger::traverse::{MultiEraBlock, MultiEraOutput};
use self_cell::self_cell;

/// Context for locating an output in the archive (for reference script pointer).
#[derive(Debug, Clone)]
pub struct OutputPointerContext {
    pub slot: BlockSlot,
    pub tx_hash: Vec<u8>,
    pub output_index: u32,
}
use std::sync::Arc;

self_cell!(
    pub struct OwnedMultiEraBlock {
        owner: Arc<BlockBody>,

        #[covariant]
        dependent: MultiEraBlock,
    }
);

impl OwnedMultiEraBlock {
    pub fn decode(buf: Arc<BlockBody>) -> Result<Self, pallas::ledger::traverse::Error> {
        Self::try_new(buf, |x| MultiEraBlock::decode(x))
    }

    pub fn view(&self) -> &MultiEraBlock<'_> {
        self.borrow_dependent()
    }
}

impl dolos_core::Block for OwnedMultiEraBlock {
    fn depends_on(&self, loaded: &mut RawUtxoMap) -> Vec<TxoRef> {
        crate::utxoset::compute_block_dependencies(self.view(), loaded)
    }

    fn slot(&self) -> BlockSlot {
        self.view().slot()
    }

    fn hash(&self) -> BlockHash {
        self.view().hash()
    }

    fn raw(&self) -> RawBlock {
        self.borrow_owner().clone()
    }
}


self_cell!(
    pub struct OwnedMultiEraOutput {
        owner: Arc<EraCbor>,

        #[not_covariant]
        dependent: MultiEraOutput,
    }
);

impl OwnedMultiEraOutput {
    // Optionally store pointer context for this output
    pub fn with_pointer_context(self, context: OutputPointerContext) -> OwnedMultiEraOutputWithContext {
        OwnedMultiEraOutputWithContext {
            output: self,
            context: Some(context),
        }
    }
}

/// Wrapper for OwnedMultiEraOutput with optional pointer context
pub struct OwnedMultiEraOutputWithContext {
    pub output: OwnedMultiEraOutput,
    pub context: Option<OutputPointerContext>,
}

impl std::fmt::Debug for OwnedMultiEraOutputWithContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnedMultiEraOutputWithContext")
            .field("context", &self.context)
            .finish()
    }
}

impl OwnedMultiEraOutputWithContext {
    pub fn pointer_context(&self) -> Option<(BlockSlot, Vec<u8>, u32)> {
        self.context.as_ref().map(|ctx| (ctx.slot, ctx.tx_hash.clone(), ctx.output_index))
    }
    pub fn inner(&self) -> &OwnedMultiEraOutput {
        &self.output
    }
}

impl OwnedMultiEraOutput {
    pub fn decode(buf: Arc<EraCbor>) -> Result<Self, pallas::ledger::traverse::Error> {
        Self::try_new(buf, |x| {
            let EraCbor(era, cbor) = x.as_ref();

            let era = pallas::ledger::traverse::Era::try_from(*era)?;

            let dec = MultiEraOutput::decode(era, cbor)
                .map_err(|x| pallas::ledger::traverse::Error::InvalidCbor(x.to_string()))?;

            Ok(dec)
        })
    }
}
