//! Integer rectangles in texel space (half-open: `x0..x1`, `y0..y1`).

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct IRect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl IRect {
    pub const EMPTY: IRect = IRect { x0: 0, y0: 0, x1: 0, y1: 0 };

    pub const fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    pub fn from_size(w: u32, h: u32) -> Self {
        Self::new(0, 0, w as i32, h as i32)
    }

    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    pub fn width(&self) -> i32 {
        (self.x1 - self.x0).max(0)
    }

    pub fn height(&self) -> i32 {
        (self.y1 - self.y0).max(0)
    }

    pub fn intersect(&self, o: &IRect) -> IRect {
        IRect::new(self.x0.max(o.x0), self.y0.max(o.y0), self.x1.min(o.x1), self.y1.min(o.y1))
    }

    pub fn union(&self, o: &IRect) -> IRect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        IRect::new(self.x0.min(o.x0), self.y0.min(o.y0), self.x1.max(o.x1), self.y1.max(o.y1))
    }

    pub fn expand(&self, by: i32) -> IRect {
        IRect::new(self.x0 - by, self.y0 - by, self.x1 + by, self.y1 + by)
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersect_and_union() {
        let a = IRect::new(0, 0, 10, 10);
        let b = IRect::new(5, 5, 15, 15);
        assert_eq!(a.intersect(&b), IRect::new(5, 5, 10, 10));
        assert_eq!(a.union(&b), IRect::new(0, 0, 15, 15));
        assert!(a.intersect(&IRect::new(20, 20, 30, 30)).is_empty());
        assert_eq!(IRect::EMPTY.union(&a), a);
    }
}
