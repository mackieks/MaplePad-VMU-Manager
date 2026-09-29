use eframe::egui::{self, Color32, Pos2, Rect, TextureHandle, Vec2};

pub struct Scenery {
    branch: TextureHandle,
    leaf: TextureHandle,
    particles: Vec<Leaf>,
    last_time: Option<f64>,
    last_bounds: Vec2,
    next_seed: usize,
}

impl Scenery {
    pub fn new(ctx: &egui::Context) -> Self {
        let load = |name, bytes| {
            let image = image::load_from_memory(bytes)
                .expect("embedded maple artwork")
                .to_rgba8();
            ctx.load_texture(
                name,
                egui::ColorImage::from_rgba_unmultiplied(
                    [image.width() as usize, image.height() as usize],
                    image.as_raw(),
                ),
                egui::TextureOptions::LINEAR,
            )
        };
        // Use the supplied leaf silhouette as an alpha mask; tinting can then vary its hue.
        let mut leaf_image = image::load_from_memory(include_bytes!("../assets/maple-icon.png"))
            .expect("leaf")
            .to_rgba8();
        for pixel in leaf_image.pixels_mut() {
            pixel.0[..3].fill(255);
        }
        let leaf = ctx.load_texture(
            "falling-maple",
            egui::ColorImage::from_rgba_unmultiplied(
                [leaf_image.width() as usize, leaf_image.height() as usize],
                leaf_image.as_raw(),
            ),
            egui::TextureOptions::LINEAR,
        );
        Self {
            leaf,
            particles: Vec::new(),
            last_time: None,
            last_bounds: Vec2::ZERO,
            next_seed: 0,
            branch: load(
                "maple-branch",
                include_bytes!("../assets/maple-branch.png").as_slice(),
            ),
        }
    }

    pub fn header(&self, painter: &egui::Painter, rect: Rect) {
        // Flip the supplied branch so its trunk emerges from the right edge.
        let painter = painter.with_clip_rect(rect);
        let size = Vec2::new(rect.width(), rect.width() * 0.56);
        // In this reference the cut trunk is at x=0.075, y=0.758.
        // Anchor that point at the right edge, inside the Source bar, while
        // letting the foliage extend up and left across both header rows.
        let image = Rect::from_min_size(
            Pos2::new(
                rect.left() + 8.0,
                rect.top() + crate::titlebar::HEIGHT + 43.0 - size.y * 0.758,
            ),
            size,
        );
        let uv = Rect::from_min_max(Pos2::new(1.0, 0.0), Pos2::new(0.07, 1.0));
        // Sample the PNG's own alpha silhouette for a soft shadow. All passes
        // remain behind both the branch and the header's foreground text.
        for n in 0..12 {
            let angle = n as f32 * std::f32::consts::TAU / 12.0;
            let offset = egui::vec2(1.5 + angle.cos() * 2.0, 2.5 + angle.sin() * 2.0);
            painter.image(
                self.branch.id(),
                image.translate(offset),
                uv,
                Color32::from_black_alpha(12),
            );
        }
        painter.image(self.branch.id(), image, uv, Color32::from_white_alpha(185));
    }

    pub fn background(&mut self, painter: &egui::Painter, rect: Rect, time: f64, light: bool) {
        let dt = self
            .last_time
            .map_or(0.0, |last| (time - last).max(0.0).min(0.1) as f32);
        self.last_time = Some(time);
        update_particles(
            &mut self.particles,
            &mut self.next_seed,
            self.last_bounds,
            rect.size(),
            dt,
        );
        self.last_bounds = rect.size();
        let painter = painter.with_clip_rect(rect);
        for leaf in &mut self.particles {
            let center = rect.min + leaf.position;
            let mut mesh = egui::Mesh::with_texture(self.leaf.id());
            let rotation = egui::emath::Rot2::from_angle(leaf.angle);
            let flutter = 0.5 + 0.5 * leaf.phase.cos().abs();
            for (corner, uv) in [
                ([-1.0, -1.0], [0.0, 0.0]),
                ([1.0, -1.0], [1.0, 0.0]),
                ([1.0, 1.0], [1.0, 1.0]),
                ([-1.0, 1.0], [0.0, 1.0]),
            ] {
                mesh.vertices.push(egui::epaint::Vertex {
                    pos: center
                        + rotation
                            * egui::vec2(
                                corner[0] * leaf.size * flutter / 2.0,
                                corner[1] * leaf.size / 2.0,
                            ),
                    uv: Pos2::new(uv[0], uv[1]),
                    color: Color32::from_rgba_unmultiplied(
                        leaf.color[0],
                        leaf.color[1],
                        leaf.color[2],
                        if light { 48 } else { 20 },
                    ),
                });
            }
            mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
            painter.add(egui::Shape::mesh(mesh));
        }
    }
}

struct Leaf {
    position: Vec2,
    phase: f32,
    angle: f32,
    size: f32,
    speed: f32,
    spin: f32,
    color: [u8; 3],
    seed: u64,
}
impl Leaf {
    fn random(&mut self) -> f32 {
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        (self.seed >> 32) as u32 as f32 / u32::MAX as f32
    }
    fn new(n: usize, bounds: Vec2) -> Self {
        let mut leaf = Self {
            position: Vec2::ZERO,
            phase: n as f32,
            angle: n as f32,
            size: 22.0 + (n % 5) as f32 * 4.0,
            speed: 18.0 + (n * 7 % 21) as f32,
            spin: 0.23 + (n % 4) as f32 * 0.07,
            color: [[205, 24, 35], [218, 42, 25], [215, 58, 22], [194, 24, 38]][n % 4],
            seed: n as u64 + 17,
        };
        leaf.position = egui::vec2(leaf.random() * bounds.x, leaf.random() * bounds.y);
        leaf
    }
    fn advance(&mut self, dt: f32) {
        // Trajectories remain in client coordinates; resizing never rescales them.
        self.phase += dt * 0.75;
        self.position.x += (5.0 + self.phase.sin() * 9.0) * dt;
        self.position.y += self.speed * dt;
        self.angle += self.spin * dt;
    }
}

fn update_particles(
    particles: &mut Vec<Leaf>,
    next_seed: &mut usize,
    old: Vec2,
    bounds: Vec2,
    dt: f32,
) {
    let target = (bounds.x * bounds.y / 47_400.0).ceil().clamp(8.0, 160.0) as usize;
    let mut i = 0;
    while i < particles.len() {
        let leaf = &mut particles[i];
        leaf.advance(dt);
        // Always use the live viewport, never the size at the leaf's birth.
        if leaf.position.y > bounds.y + 45.0 {
            if particles.len() > target {
                particles.swap_remove(i);
                continue;
            }
            let leaf = &mut particles[i];
            leaf.position = egui::vec2(leaf.random() * bounds.x, -45.0);
        }
        i += 1;
    }
    // Keep a roughly constant density. Populate newly exposed right/bottom
    // strips without redistributing any leaves already on screen.
    while particles.len() < target {
        let mut leaf = Leaf::new(*next_seed, bounds);
        *next_seed += 1;
        if old.x > 0.0 && old.y > 0.0 {
            let right_area = (bounds.x - old.x).max(0.0) * bounds.y;
            let bottom_area = bounds.x.min(old.x) * (bounds.y - old.y).max(0.0);
            if right_area + bottom_area > 0.0 {
                leaf.position = if leaf.random() * (right_area + bottom_area) < right_area {
                    egui::vec2(
                        old.x + leaf.random() * (bounds.x - old.x),
                        leaf.random() * bounds.y,
                    )
                } else {
                    egui::vec2(
                        leaf.random() * bounds.x.min(old.x),
                        old.y + leaf.random() * (bounds.y - old.y),
                    )
                };
            }
        }
        particles.push(leaf);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resizing_does_not_move_or_accelerate_existing_leaves() {
        let mut a = Leaf::new(3, egui::vec2(1200.0, 830.0));
        let mut b = Leaf::new(3, egui::vec2(1200.0, 830.0));
        let before = a.position;
        a.advance(0.0);
        assert_eq!(a.position, before);
        for _ in 0..30 {
            a.advance(0.033);
            b.advance(0.033);
            assert_eq!(a.position, b.position);
            assert_eq!(a.angle, b.angle);
        }
    }
    #[test]
    fn expanding_populates_new_space_and_recycles_at_the_live_bottom() {
        let old = egui::vec2(1200.0, 830.0);
        let expanded = egui::vec2(1800.0, 1300.0);
        let mut particles = Vec::new();
        let mut seed = 0;
        update_particles(&mut particles, &mut seed, Vec2::ZERO, old, 0.0);
        particles[0].position.y = 880.0; // Beyond the old bottom, within the new one.
        let before: Vec<_> = particles.iter().map(|p| p.position).collect();
        update_particles(&mut particles, &mut seed, old, expanded, 0.0);
        for (leaf, pos) in particles.iter().zip(&before) {
            assert_eq!(leaf.position, *pos);
        }
        assert!(particles.len() > before.len());
        assert!(particles[before.len()..]
            .iter()
            .all(|p| p.position.x >= old.x || p.position.y >= old.y));
        assert!(particles[before.len()..]
            .iter()
            .any(|p| p.position.x >= old.x));
        assert!(particles[before.len()..]
            .iter()
            .any(|p| p.position.y >= old.y));
        particles[0].position.y = expanded.y + 46.0;
        update_particles(&mut particles, &mut seed, expanded, expanded, 0.0);
        assert_eq!(particles[0].position.y, -45.0);
    }
}
