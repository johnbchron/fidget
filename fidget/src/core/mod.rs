//! Core infrastructure for evaluating complex closed-form implicit surfaces.
//!
//! ```
//! use fidget::{vm, context::Context};
//! let mut ctx = Context::new();
//! let x = ctx.x();
//! let y = ctx.y();
//! let x_squared = ctx.mul(x, x)?;
//! let y_squared = ctx.mul(y, y)?;
//! let radius = ctx.add(x_squared, y_squared)?;
//! let circle = ctx.sub(radius, 1.0)?;
//!
//! let tape = ctx.get_tape::<vm::Eval>(circle)?;
//! let mut eval = tape.new_point_evaluator();
//! assert_eq!(eval.eval(0.0, 0.0, 0.0, &[])?.0, -1.0);
//! assert_eq!(eval.eval(1.0, 0.0, 0.0, &[])?.0, 0.0);
//!
//! const N: usize = 15;
//! for i in 0..N {
//!     for j in 0..N {
//!         let x = (i as f32 + 0.5) / (N as f32 / 2.0) - 1.0;
//!         let y = (j as f32 + 0.5) / (N as f32 / 2.0) - 1.0;
//!         let v = eval.eval(x, y, 0.0, &[])?.0;
//!         print!("{}", if v < 0.0 { "XX" } else { "  " });
//!     }
//!     println!();
//! }
//!
//! // This will print
//! //           XXXXXXXXXX
//! //       XXXXXXXXXXXXXXXXXX
//! //     XXXXXXXXXXXXXXXXXXXXXX
//! //   XXXXXXXXXXXXXXXXXXXXXXXXXX
//! //   XXXXXXXXXXXXXXXXXXXXXXXXXX
//! // XXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
//! // XXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
//! // XXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
//! // XXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
//! // XXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
//! //   XXXXXXXXXXXXXXXXXXXXXXXXXX
//! //   XXXXXXXXXXXXXXXXXXXXXXXXXX
//! //     XXXXXXXXXXXXXXXXXXXXXX
//! //       XXXXXXXXXXXXXXXXXX
//! //           XXXXXXXXXX
//! # Ok::<(), fidget::Error>(())
//! ```
pub mod context;
pub use context::Context;

pub mod eval;
pub mod ssa;
pub mod vm;

#[cfg(test)]
mod test {
    use crate::context::*;

    #[test]
    fn it_works() {
        let mut ctx = Context::new();
        let x1 = ctx.x();
        let x2 = ctx.x();
        assert_eq!(x1, x2);

        let a = ctx.constant(1.0);
        let b = ctx.constant(1.0);
        assert_eq!(a, b);
        assert_eq!(ctx.const_value(a).unwrap(), Some(1.0));
        assert_eq!(ctx.const_value(x1).unwrap(), None);

        let c = ctx.add(a, b).unwrap();
        assert_eq!(ctx.const_value(c).unwrap(), Some(2.0));

        let c = ctx.neg(c).unwrap();
        assert_eq!(ctx.const_value(c).unwrap(), Some(-2.0));
    }

    #[test]
    fn test_constant_folding() {
        let mut ctx = Context::new();
        let a = ctx.constant(1.0);
        assert_eq!(ctx.len(), 1);
        let b = ctx.constant(-1.0);
        assert_eq!(ctx.len(), 2);
        let _ = ctx.add(a, b);
        assert_eq!(ctx.len(), 3);
        let _ = ctx.add(a, b);
        assert_eq!(ctx.len(), 3);
        let _ = ctx.mul(a, b);
        assert_eq!(ctx.len(), 3);
    }

    #[test]
    fn test_eval() {
        let mut ctx = Context::new();
        let x = ctx.x();
        let y = ctx.y();
        let v = ctx.add(x, y).unwrap();

        assert_eq!(
            ctx.eval(
                v,
                &[("X".to_string(), 1.0), ("Y".to_string(), 2.0)]
                    .into_iter()
                    .collect()
            )
            .unwrap(),
            3.0
        );
        assert_eq!(ctx.eval_xyz(v, 2.0, 3.0, 0.0).unwrap(), 5.0);
    }
}
