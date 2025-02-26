//! Instruction tapes in the form of assembly for a simple virtual machine
mod alloc;
mod eval;
mod lru;
mod op;
mod tape;

pub(super) use alloc::RegisterAllocator;

pub use eval::Eval;
pub use op::Op;
pub use tape::Tape;
