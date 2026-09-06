//! A reverse Dijkstra field shared by a whole formation, built incrementally.
//! Sampling is bounded; every accepted edge still uses canonical collision.
use bevy::prelude::*;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

const MAX_SIDE: usize = 128;
const MARGIN: f32 = 24.0;
const NEIGHBORS: [(i32, i32, u32); 8] = [
    (-1, 0, 1000),
    (1, 0, 1000),
    (0, -1, 1000),
    (0, 1, 1000),
    (-1, -1, 1414),
    (-1, 1, 1414),
    (1, -1, 1414),
    (1, 1, 1414),
];

pub(super) struct FlowField {
    origin: Vec2,
    step: f32,
    width: usize,
    height: usize,
    cost: Vec<u32>,
    parent: Vec<usize>,
    frontier: BinaryHeap<Reverse<(u32, usize)>>,
}
impl FlowField {
    pub fn new(points: &[Vec2], goal: Vec2) -> Self {
        let min = points.iter().fold(goal, |a, b| a.min(*b)) - Vec2::splat(MARGIN);
        let max = points.iter().fold(goal, |a, b| a.max(*b)) + Vec2::splat(MARGIN);
        let step = ((max - min).max_element() / (MAX_SIDE - 1) as f32).max(2.0);
        // Align the root exactly to the goal, so it needs no uncertified link.
        let origin = goal - ((goal - min) / step).floor() * step;
        let width = (((max.x - origin.x) / step).ceil() as usize + 1).min(MAX_SIDE + 1);
        let height = (((max.y - origin.y) / step).ceil() as usize + 1).min(MAX_SIDE + 1);
        let mut result = Self {
            origin,
            step,
            width,
            height,
            cost: vec![u32::MAX; width * height],
            parent: vec![usize::MAX; width * height],
            frontier: BinaryHeap::new(),
        };
        let root = result.index(goal).expect("goal inside field");
        result.cost[root] = 0;
        result.parent[root] = root;
        result.frontier.push(Reverse((0, root)));
        result
    }
    fn point(&self, index: usize) -> Vec2 {
        self.origin
            + Vec2::new((index % self.width) as f32, (index / self.width) as f32) * self.step
    }
    fn index(&self, point: Vec2) -> Option<usize> {
        let p = ((point - self.origin) / self.step).round().as_ivec2();
        self.cell(p.x, p.y)
    }
    fn cell(&self, x: i32, y: i32) -> Option<usize> {
        (x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height)
            .then(|| y as usize * self.width + x as usize)
    }
    pub fn complete(&self) -> bool {
        self.frontier.is_empty()
    }
    #[cfg(test)]
    pub fn advance(&mut self, budget: &mut usize, clear: &impl Fn(Vec2, Vec2) -> bool) {
        self.advance_until(
            budget,
            std::time::Instant::now() + std::time::Duration::from_secs(60),
            clear,
        );
    }
    pub fn advance_until(
        &mut self,
        budget: &mut usize,
        deadline: std::time::Instant,
        clear: &impl Fn(Vec2, Vec2) -> bool,
    ) {
        while *budget > 0 {
            if (*budget % 16) == 0 && std::time::Instant::now() >= deadline {
                break;
            }
            let Some(Reverse((cost, index))) = self.frontier.pop() else {
                break;
            };
            *budget -= 1;
            if cost != self.cost[index] {
                continue;
            }
            let from = self.point(index);
            for (dx, dy, distance) in NEIGHBORS {
                let Some(next) = self.cell(
                    (index % self.width) as i32 + dx,
                    (index / self.width) as i32 + dy,
                ) else {
                    continue;
                };
                let proposed = cost + distance;
                if proposed >= self.cost[next] || !clear(from, self.point(next)) {
                    continue;
                }
                self.cost[next] = proposed;
                self.parent[next] = index;
                self.frontier.push(Reverse((proposed, next)));
            }
        }
    }
    /// Extract a gradient path; this does no search and shares the field's work.
    pub fn path(
        &self,
        start: Vec2,
        goal: Vec2,
        clear: &impl Fn(Vec2, Vec2) -> bool,
    ) -> Option<Vec<Vec2>> {
        let nearest = self.index(start)?;
        let x = (nearest % self.width) as i32;
        let y = (nearest / self.width) as i32;
        let mut choices = Vec::with_capacity(9);
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(i) = self.cell(x + dx, y + dy) {
                    if self.cost[i] != u32::MAX && clear(start, self.point(i)) {
                        choices.push(i);
                    }
                }
            }
        }
        let mut at = choices.into_iter().min_by_key(|i| self.cost[*i])?;
        let mut path = Vec::new();
        for _ in 0..self.parent.len() {
            let point = self.point(at);
            path.push(point);
            if clear(point, goal) {
                path.push(goal);
                return Some(path);
            }
            let next = self.parent[at];
            if next == at || next == usize::MAX {
                return None;
            }
            at = next;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_budgeted_field_routes_fifty_soldiers_around_a_wall() {
        let clear = |a: Vec2, b: Vec2| {
            // Exact intersection with a closed wall segment x=0, z=-8..8.
            if a.x * b.x > 0.0 {
                return true;
            }
            if (b.x - a.x).abs() < 0.001 {
                return a.x.abs() > 0.001 || a.y.min(b.y) > 8.0 || a.y.max(b.y) < -8.0;
            }
            let y = a.y + (b.y - a.y) * (-a.x / (b.x - a.x));
            y.abs() > 8.0
        };
        let starts: Vec<_> = (0..50)
            .map(|i| Vec2::new(-20.0 - (i / 10) as f32, (i % 10) as f32 - 4.5))
            .collect();
        let goal = Vec2::new(20.0, 0.0);
        let mut field = FlowField::new(&starts, goal);
        field.advance(&mut 1, &clear);
        assert!(!field.complete(), "a tick respects the expansion budget");
        while !field.complete() {
            field.advance(&mut 128, &clear);
        }
        for start in starts {
            let path = field.path(start, goal, &clear).unwrap();
            let mut last = start;
            for point in path {
                assert!(clear(last, point));
                last = point;
            }
            assert_eq!(last, goal);
        }
    }
}
