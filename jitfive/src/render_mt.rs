use crate::{
    asm::dynasm::{
        build_interval_fn, build_vec_fn, IntervalEval, IntervalFuncHandle,
        VecEval,
    },
    render::Pixel,
    tape::Tape,
};
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct RenderConfig {
    pub image_size: usize,
    pub tile_size: usize,
    pub subtile_size: usize,
    pub interval_subdiv: usize,
    pub threads: usize,
}

impl RenderConfig {
    fn pixel_to_pos(&self, p: usize) -> f32 {
        2.0 * (p as f32) / (self.image_size as f32) - 1.0
    }
}

#[derive(Copy, Clone, Debug)]
struct Tile {
    corner: [usize; 2],
}

fn worker(
    i_handle: &IntervalFuncHandle,
    tiles: &[Tile],
    i: &AtomicUsize,
    config: &RenderConfig,
) -> Vec<(Tile, Vec<Pixel>)> {
    let mut eval = i_handle.get_evaluator();
    let mut out = vec![];
    loop {
        let index = i.fetch_add(1, Ordering::Relaxed);
        if index >= tiles.len() {
            break;
        }
        let tile = tiles[index];

        let mut pixels = vec![None; config.image_size * config.image_size];
        render_tile(&mut eval, &mut pixels, config, tile);
        let pixels = pixels.into_iter().map(Option::unwrap).collect();
        out.push((tile, pixels))
    }
    out
}

fn render_tile(
    eval: &mut IntervalEval,
    out: &mut [Option<Pixel>],
    config: &RenderConfig,
    tile: Tile,
) {
    println!("rendering {:?}", tile);
    let x_min = config.pixel_to_pos(tile.corner[0]);
    let x_max = config.pixel_to_pos(tile.corner[0] + config.tile_size);
    let y_min = config.pixel_to_pos(tile.corner[1]);
    let y_max = config.pixel_to_pos(tile.corner[1] + config.tile_size);

    let i = eval.eval_subdiv(
        [x_min, x_max],
        [y_min, y_max],
        [0.0, 0.0],
        config.interval_subdiv,
    );

    if i[1] < 0.0 {
        for y in 0..config.tile_size {
            for x in 0..config.tile_size {
                out[x + y * config.tile_size] = Some(Pixel::FilledTile);
            }
        }
    } else if i[0] > 0.0 {
        for y in 0..config.tile_size {
            for x in 0..config.tile_size {
                out[x + y * config.tile_size] = Some(Pixel::EmptyTile);
            }
        }
    } else {
        let sub_tape = eval.push();
        let sub_jit = build_interval_fn(&sub_tape);
        let mut sub_eval = sub_jit.get_evaluator();
        let n = config.tile_size / config.subtile_size;
        for j in 0..n {
            for i in 0..n {
                render_subtile(
                    &mut sub_eval,
                    out,
                    config,
                    Tile {
                        corner: [
                            tile.corner[0] + i * config.subtile_size,
                            tile.corner[1] + j * config.subtile_size,
                        ],
                    },
                );
            }
        }
    }
}

fn render_subtile(
    eval: &mut IntervalEval,
    out: &mut [Option<Pixel>],
    config: &RenderConfig,
    tile: Tile,
) {
    println!("    rendering {:?}", tile);
    let x_min = config.pixel_to_pos(tile.corner[0]);
    let x_max = config.pixel_to_pos(tile.corner[0] + config.subtile_size);
    let y_min = config.pixel_to_pos(tile.corner[1]);
    let y_max = config.pixel_to_pos(tile.corner[1] + config.subtile_size);

    let i = eval.eval_subdiv(
        [x_min, x_max],
        [y_min, y_max],
        [0.0, 0.0],
        config.interval_subdiv,
    );
    println!("    {:?}", i);

    if i[1] < 0.0 {
        for y in 0..config.subtile_size {
            for x in 0..config.subtile_size {
                out[x + y * config.subtile_size] = Some(Pixel::FilledSubtile);
            }
        }
    } else if i[0] > 0.0 {
        for y in 0..config.subtile_size {
            for x in 0..config.subtile_size {
                out[x + y * config.subtile_size] = Some(Pixel::EmptySubtile);
            }
        }
    } else {
        let sub_tape = eval.push();
        let sub_jit = build_vec_fn(&sub_tape);
        let mut sub_eval = sub_jit.get_evaluator();
        for j in 0..config.subtile_size {
            for i in 0..(config.subtile_size / 4) {
                render_pixels(
                    &mut sub_eval,
                    out,
                    config,
                    Tile {
                        corner: [tile.corner[0] + i * 4, tile.corner[1] + j],
                    },
                );
            }
        }
    }
}

fn render_pixels(
    eval: &mut VecEval,
    out: &mut [Option<Pixel>],
    config: &RenderConfig,
    tile: Tile,
) {
    println!("        rendering {:?}", tile);
    let mut x_vec = [0.0; 4];
    for (i, x) in x_vec.iter_mut().enumerate() {
        *x = config.pixel_to_pos(tile.corner[0] + i);
    }
    let y_vec = [config.pixel_to_pos(tile.corner[1]); 4];
    let v = eval.eval(x_vec, y_vec, [0.0; 4]);

    for (i, v) in v.iter().enumerate() {
        out[tile.corner[0] + i + tile.corner[1] * config.subtile_size] =
            Some(if *v < 0.0 {
                Pixel::Filled
            } else {
                Pixel::Empty
            });
    }
}

pub fn render(tape: &Tape, config: &RenderConfig) -> Vec<Pixel> {
    assert!(config.image_size % config.tile_size == 0);
    assert!(config.tile_size % config.subtile_size == 0);
    assert!(config.subtile_size % 4 == 0);

    let i_handle = build_interval_fn(tape);
    let mut tiles = vec![];
    for i in 0..config.image_size / config.tile_size {
        for j in 0..config.image_size / config.tile_size {
            tiles.push(Tile { corner: [i, j] });
        }
    }

    let index = AtomicUsize::new(0);
    let out = std::thread::scope(|s| {
        let mut handles = vec![];
        for _ in 0..config.threads {
            handles.push(s.spawn(|| worker(&i_handle, &tiles, &index, config)));
        }
        let mut out = vec![];
        for h in handles {
            out.extend(h.join().unwrap().into_iter());
        }
        out
    });

    let mut image = vec![None; config.image_size * config.image_size];
    for (tile, data) in out.iter() {
        for j in 0..config.tile_size {
            for i in 0..config.tile_size {
                let x = i + tile.corner[0];
                let y = j + tile.corner[1];
                image[x + y * config.image_size] =
                    Some(data[i + j * config.tile_size]);
            }
        }
    }
    image.into_iter().map(Option::unwrap).collect()
}
