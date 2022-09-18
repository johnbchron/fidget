use crate::context::{indexed::Index, Node, VarNode};
use ordered_float::OrderedFloat;

/// A one-argument math operation
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub enum UnaryOpcode {
    Neg,
    Abs,
    Recip,
    Sqrt,
    Square,
}

/// A two-argument math operation
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub enum BinaryOpcode {
    Add,
    Mul,
    Sub,
    Min,
    Max,
}

/// Represents an operation in a math expression.
///
/// `Op`s should be constructed by calling functions on
/// [`Context`](crate::context::Context), e.g.
/// [`Context::add`](crate::context::Context::add) will generate an `Op::Add`
/// node and return an opaque handle.
///
/// Each `Op` is tightly coupled to the [`Context`](crate::context::Context)
/// which generated it, and will not be valid for a different `Context`.
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub enum Op {
    Var(VarNode),
    Const(OrderedFloat<f64>),
    Binary(BinaryOpcode, Node, Node),
    Unary(UnaryOpcode, Node),
}

fn dot_color_to_rgb(s: &str) -> &'static str {
    match s {
        "red" => "#FF0000",
        "green" => "#00FF00",
        "goldenrod" => "#DAA520",
        "dodgerblue" => "#1E90FF",
        s => panic!("Unknown X11 color '{s}'"),
    }
}

impl Op {
    pub fn dot_node_color(&self) -> &str {
        match self {
            Op::Const(..) => "green",
            Op::Var(..) => "red",
            Op::Binary(BinaryOpcode::Min | BinaryOpcode::Max, ..) => {
                "dodgerblue"
            }
            Op::Binary(..) | Op::Unary(..) => "goldenrod",
        }
    }
    pub fn dot_node_shape(&self) -> &str {
        match self {
            Op::Const(..) => "oval",
            Op::Var(..) => "circle",
            Op::Binary(..) | Op::Unary(..) => "box",
        }
    }

    pub fn iter_children(&self) -> impl Iterator<Item = Node> {
        let out = match self {
            Op::Binary(_, a, b) => [Some(*a), Some(*b)],
            Op::Unary(_, a) => [Some(*a), None],
            Op::Var(..) | Op::Const(..) => [None, None],
        };
        out.into_iter().flatten()
    }
}

impl Op {
    pub fn dot_edges(&self, i: Node) -> String {
        let mut out = String::new();
        for c in self.iter_children() {
            out += &self.dot_edge(i, c, "FF");
        }
        out
    }

    pub fn dot_edge(&self, a: Node, b: Node, alpha: &str) -> String {
        let color = dot_color_to_rgb(self.dot_node_color()).to_owned() + alpha;
        format!("n{} -> n{} [color = \"{color}\"]\n", a.get(), b.get(),)
    }
}
