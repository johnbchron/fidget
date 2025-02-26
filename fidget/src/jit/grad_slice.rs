use crate::jit::{AssemblerData, JitBulkEval, SimdAssembler};

/// Assembler for automatic differentiation / gradient evaluation
pub struct GradSliceAssembler(pub(crate) AssemblerData<[f32; 4]>);
pub type JitGradSliceEval = JitBulkEval<GradSliceAssembler>;

// Both x86_64 and AArch64 process 1 gradient per register
impl SimdAssembler for GradSliceAssembler {
    const SIMD_SIZE: usize = 1;
}
