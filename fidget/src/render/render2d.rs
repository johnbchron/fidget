//! Bitmap rendering
use crate::{
    eval::{
        float_slice::{FloatSliceEval, FloatSliceEvalT},
        interval::{Interval, IntervalEval, IntervalEvalT},
        tape::{Tape, TapeData, Workspace},
        Eval,
    },
    render::config::{AlignedRenderConfig, Queue, RenderConfig, Tile},
};
use nalgebra::{Point2, Vector2};

////////////////////////////////////////////////////////////////////////////////

/// Configuration trait for rendering
pub trait RenderMode {
    /// Type of output pixel
    type Output: Default + Copy + Clone + Send;

    /// Decide whether to subdivide or fill an interval
    fn interval(i: Interval, depth: usize) -> Option<Self::Output>;

    /// Per-pixel drawing
    fn pixel(f: f32) -> Self::Output;
}

////////////////////////////////////////////////////////////////////////////////

/// Renderer that emits `DebugPixel`
pub struct DebugRenderMode;

impl RenderMode for DebugRenderMode {
    type Output = DebugPixel;
    fn interval(i: Interval, depth: usize) -> Option<DebugPixel> {
        if i.upper() < 0.0 {
            if depth > 1 {
                Some(DebugPixel::FilledSubtile)
            } else {
                Some(DebugPixel::FilledTile)
            }
        } else if i.lower() > 0.0 {
            if depth > 1 {
                Some(DebugPixel::EmptySubtile)
            } else {
                Some(DebugPixel::EmptyTile)
            }
        } else {
            None
        }
    }
    fn pixel(f: f32) -> DebugPixel {
        if f < 0.0 {
            DebugPixel::Filled
        } else {
            DebugPixel::Empty
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum DebugPixel {
    EmptyTile,
    FilledTile,
    EmptySubtile,
    FilledSubtile,
    Empty,
    Filled,
    Invalid,
}

impl Default for DebugPixel {
    fn default() -> Self {
        DebugPixel::Invalid
    }
}

impl DebugPixel {
    #[inline]
    pub fn as_debug_color(&self) -> [u8; 4] {
        match self {
            DebugPixel::EmptyTile => [50, 0, 0, 255],
            DebugPixel::FilledTile => [255, 0, 0, 255],
            DebugPixel::EmptySubtile => [0, 50, 0, 255],
            DebugPixel::FilledSubtile => [0, 255, 0, 255],
            DebugPixel::Empty => [0, 0, 0, 255],
            DebugPixel::Filled => [255, 255, 255, 255],
            DebugPixel::Invalid => panic!(),
        }
    }

    #[inline]
    pub fn is_filled(&self) -> bool {
        match self {
            DebugPixel::EmptyTile
            | DebugPixel::EmptySubtile
            | DebugPixel::Empty => false,
            DebugPixel::FilledTile
            | DebugPixel::FilledSubtile
            | DebugPixel::Filled => true,
            DebugPixel::Invalid => panic!(),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////

/// Renderer that emits `bool`
pub struct BitRenderMode;

impl RenderMode for BitRenderMode {
    type Output = bool;
    fn interval(i: Interval, _depth: usize) -> Option<bool> {
        if i.upper() < 0.0 {
            Some(true)
        } else if i.lower() > 0.0 {
            Some(false)
        } else {
            None
        }
    }
    fn pixel(f: f32) -> bool {
        f < 0.0
    }
}

////////////////////////////////////////////////////////////////////////////////

struct Scratch {
    x: Vec<f32>,
    y: Vec<f32>,
    z: Vec<f32>,
    out: Vec<f32>,
}

impl Scratch {
    fn new(size: usize) -> Self {
        Self {
            x: vec![0.0; size],
            y: vec![0.0; size],
            z: vec![0.0; size],
            out: vec![0.0; size],
        }
    }
}

////////////////////////////////////////////////////////////////////////////////

struct Worker<'a, I: Eval, M: RenderMode> {
    config: &'a AlignedRenderConfig<2>,
    scratch: Scratch,

    image: Vec<M::Output>,

    /// Storage for float slice evaluators
    float_storage:
        [<<I as Eval>::FloatSliceEval as FloatSliceEvalT<I>>::Storage; 2],

    /// Storage for interval evaluators, based on recursion depth
    interval_storage:
        Vec<<<I as Eval>::IntervalEval as IntervalEvalT<I>>::Storage>,

    spare_tapes: Vec<TapeData>,
    workspace: Workspace,
}

impl<I: Eval, M: RenderMode> Worker<'_, I, M> {
    fn render_tile_recurse(
        &mut self,
        i_handle: &mut IntervalEval<I>,
        depth: usize,
        tile: Tile<2>,
        float_handle: &mut Option<FloatSliceEval<I>>,
    ) {
        let tile_size = self.config.tile_sizes[depth];

        // Brute-force way to find the (interval) bounding box of the region
        let mut x_min = f32::INFINITY;
        let mut x_max = f32::NEG_INFINITY;
        let mut y_min = f32::INFINITY;
        let mut y_max = f32::NEG_INFINITY;
        let base = Point2::from(tile.corner);
        for i in 0..4 {
            let offset = Vector2::new(
                if (i & 1) == 0 { 0 } else { tile_size },
                if (i & 2) == 0 { 0 } else { tile_size },
            );
            let p = (base + offset).cast::<f32>();
            let p = self.config.mat.transform_point(&p);
            x_min = x_min.min(p.x);
            x_max = x_max.max(p.x);
            y_min = y_min.min(p.y);
            y_max = y_max.max(p.y);
        }
        let x = Interval::new(x_min, x_max);
        let y = Interval::new(y_min, y_max);
        let z = Interval::new(0.0, 0.0);
        let i = i_handle.eval_i(x, y, z);

        let fill = M::interval(i, depth);

        if let Some(fill) = fill {
            for y in 0..tile_size {
                let start = self.config.tile_to_offset(tile, 0, y);
                self.image[start..][..tile_size].fill(fill);
            }
        } else if let Some(next_tile_size) =
            self.config.tile_sizes.get(depth + 1)
        {
            let sub_tape = i_handle.simplify_with(
                &mut self.workspace,
                std::mem::take(&mut self.spare_tapes[depth]),
            );
            let s = std::mem::take(&mut self.interval_storage[depth]);
            let mut sub_jit =
                I::new_interval_evaluator_with_storage(sub_tape.clone(), s);
            let n = tile_size / next_tile_size;
            let mut float_handle = None;
            for j in 0..n {
                for i in 0..n {
                    self.render_tile_recurse(
                        &mut sub_jit,
                        depth + 1,
                        self.config.new_tile([
                            tile.corner[0] + i * next_tile_size,
                            tile.corner[1] + j * next_tile_size,
                        ]),
                        &mut float_handle,
                    );
                }
            }
            self.interval_storage[depth] = sub_jit.take().unwrap();
            if let Some(f) = float_handle {
                self.float_storage[0] = f.take().unwrap();
            }
            self.spare_tapes[depth] = sub_tape.take().unwrap();
        } else {
            self.render_tile_pixels(i_handle, tile_size, tile, float_handle)
        }
    }
    fn render_tile_pixels(
        &mut self,
        i_handle: &mut IntervalEval<I>,
        tile_size: usize,
        tile: Tile<2>,
        float_handle: &mut Option<FloatSliceEval<I>>,
    ) {
        let mut index = 0;
        for j in 0..tile_size {
            for i in 0..tile_size {
                let p = self.config.mat.transform_point(&Point2::new(
                    (tile.corner[0] + i) as f32,
                    (tile.corner[1] + j) as f32,
                ));
                self.scratch.x[index] = p.x;
                self.scratch.y[index] = p.y;
                index += 1;
            }
        }

        let sub_tape = i_handle.simplify_with(
            &mut self.workspace,
            std::mem::take(self.spare_tapes.last_mut().unwrap()),
        );

        // In some cases, the shortened tape isn't actually any shorter, so
        // it's a waste of time to rebuild it.  Instead, we want to use a
        // float-slice evaluator that's bound to the *parent* tape.
        // Luckily, such a thing _may_ be passed into this function.  If
        // not, we build it here and then pass it out, so future calls can
        // use it.
        //
        // (this matters most for the JIT compiler, which is _expensive_)
        if sub_tape.len() < i_handle.tape().len() {
            let s = std::mem::take(&mut self.float_storage[1]);
            let mut func =
                I::new_float_slice_evaluator_with_storage(sub_tape.clone(), s);

            func.eval_s(
                &self.scratch.x,
                &self.scratch.y,
                &self.scratch.z,
                &mut self.scratch.out,
            )
            .unwrap();

            // We consume the evaluator, so any reuse of memory between the
            // FloatSliceFunc and FloatSliceEval should be cleared up and we
            // should be able to reuse the working memory.
            self.float_storage[1] = func.take().unwrap();
        } else {
            // Reuse the FloatSliceFunc handle passed in, or build one if it
            // wasn't already available (which makes it available to siblings)
            let func = float_handle.get_or_insert_with(|| {
                let s = std::mem::take(&mut self.float_storage[0]);
                I::new_float_slice_evaluator_with_storage(i_handle.tape(), s)
            });

            func.eval_s(
                &self.scratch.x,
                &self.scratch.y,
                &self.scratch.z,
                &mut self.scratch.out,
            )
            .unwrap();

            // Don't release func to self.float_storage[0] here; it's done by
            // the parent caller at the end of subtile iteration.
        }

        let mut index = 0;
        for j in 0..tile_size {
            let o = self.config.tile_to_offset(tile, 0, j);
            for i in 0..tile_size {
                self.image[o + i] = M::pixel(self.scratch.out[index]);
                index += 1;
            }
        }
        *self.spare_tapes.last_mut().unwrap() = sub_tape.take().unwrap();
    }
}

////////////////////////////////////////////////////////////////////////////////

fn worker<I: Eval, M: RenderMode>(
    mut i_handle: IntervalEval<I>,
    queue: &Queue<2>,
    config: &AlignedRenderConfig<2>,
) -> Vec<(Tile<2>, Vec<M::Output>)> {
    let mut out = vec![];
    let scratch = Scratch::new(config.tile_sizes.last().unwrap_or(&0).pow(2));

    let mut w: Worker<I, M> = Worker {
        scratch,
        image: vec![],
        config,
        float_storage: Default::default(),
        interval_storage: (0..config.tile_sizes.len())
            .map(|_| Default::default())
            .collect(),
        spare_tapes: (0..config.tile_sizes.len())
            .map(|_| Default::default())
            .collect(),
        workspace: Default::default(),
    };
    while let Some(tile) = queue.next() {
        w.image = vec![M::Output::default(); config.tile_sizes[0].pow(2)];
        w.render_tile_recurse(&mut i_handle, 0, tile, &mut None);
        let pixels = std::mem::take(&mut w.image);
        out.push((tile, pixels))
    }
    out
}

////////////////////////////////////////////////////////////////////////////////

pub fn render<I: Eval, M: RenderMode>(
    tape: Tape<I>,
    config: &RenderConfig<2>,
) -> Vec<M::Output> {
    let config = config.align();
    assert!(config.image_size % config.tile_sizes[0] == 0);
    for i in 0..config.tile_sizes.len() - 1 {
        assert!(config.tile_sizes[i] % config.tile_sizes[i + 1] == 0);
    }

    let i_handle = I::new_interval_evaluator(tape);
    let mut tiles = vec![];
    for i in 0..config.image_size / config.tile_sizes[0] {
        for j in 0..config.image_size / config.tile_sizes[0] {
            tiles.push(config.new_tile([
                i * config.tile_sizes[0],
                j * config.tile_sizes[0],
            ]));
        }
    }

    let queue = Queue::new(tiles);
    let out = std::thread::scope(|s| {
        let mut handles = vec![];
        for _ in 0..config.threads {
            let i = i_handle.clone();
            handles.push(s.spawn(|| worker::<I, M>(i, &queue, &config)));
        }
        let mut out = vec![];
        for h in handles {
            out.extend(h.join().unwrap().into_iter());
        }
        out
    });

    let mut image = vec![M::Output::default(); config.orig_image_size.pow(2)];
    for (tile, data) in out.iter() {
        let mut index = 0;
        for j in 0..config.tile_sizes[0] {
            let y = j + tile.corner[1];
            for i in 0..config.tile_sizes[0] {
                let x = i + tile.corner[0];
                if y < config.orig_image_size && x < config.orig_image_size {
                    let o = (config.orig_image_size - y - 1)
                        * config.orig_image_size
                        + x;
                    image[o] = data[index];
                }
                index += 1;
            }
        }
    }
    image
}
