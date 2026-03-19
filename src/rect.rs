use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};

/// Shared rectangle state between overlay (writer) and pipeline (reader).
/// Uses atomics for lock-free cross-thread communication.
#[derive(Debug)]
pub struct AtomicRect {
    x: AtomicI32,
    y: AtomicI32,
    width: AtomicU32,
    height: AtomicU32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl AtomicRect {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Arc<Self> {
        Arc::new(Self {
            x: AtomicI32::new(x),
            y: AtomicI32::new(y),
            width: AtomicU32::new(width),
            height: AtomicU32::new(height),
        })
    }

    pub fn get(&self) -> Rect {
        Rect {
            x: self.x.load(Ordering::Relaxed),
            y: self.y.load(Ordering::Relaxed),
            width: self.width.load(Ordering::Relaxed),
            height: self.height.load(Ordering::Relaxed),
        }
    }

    #[allow(dead_code)]
    pub fn set_position(&self, x: i32, y: i32) {
        self.x.store(x, Ordering::Relaxed);
        self.y.store(y, Ordering::Relaxed);
    }

    pub fn set_size(&self, width: u32, height: u32) {
        self.width.store(width, Ordering::Relaxed);
        self.height.store(height, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atomic_rect_new_and_get() {
        let rect = AtomicRect::new(100, 200, 1280, 720);
        let r = rect.get();
        assert_eq!(r.x, 100);
        assert_eq!(r.y, 200);
        assert_eq!(r.width, 1280);
        assert_eq!(r.height, 720);
    }

    #[test]
    fn test_atomic_rect_set_position() {
        let rect = AtomicRect::new(0, 0, 1280, 720);
        rect.set_position(500, 300);
        let r = rect.get();
        assert_eq!(r.x, 500);
        assert_eq!(r.y, 300);
        assert_eq!(r.width, 1280);
        assert_eq!(r.height, 720);
    }

    #[test]
    fn test_atomic_rect_set_size() {
        let rect = AtomicRect::new(100, 200, 1280, 720);
        rect.set_size(1920, 1080);
        let r = rect.get();
        assert_eq!(r.x, 100);
        assert_eq!(r.y, 200);
        assert_eq!(r.width, 1920);
        assert_eq!(r.height, 1080);
    }

    #[test]
    fn test_rect_equality() {
        let a = Rect {
            x: 10,
            y: 20,
            width: 100,
            height: 200,
        };
        let b = Rect {
            x: 10,
            y: 20,
            width: 100,
            height: 200,
        };
        assert_eq!(a, b);
    }
}
