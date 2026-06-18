/// A stereo audio frame. Aurio processes audio in stereo end-to-end: every
/// source produces `Frame`s, every effect transforms `Frame`s, and the graph
/// sums `Frame`s. Mono sources fill both channels equally.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Frame {
    pub l: f32,
    pub r: f32,
}

impl Frame {
    pub const SILENCE: Frame = Frame { l: 0.0, r: 0.0 };

    /// A mono value placed equally in both channels.
    #[inline]
    pub fn mono(v: f32) -> Self {
        Frame { l: v, r: v }
    }

    #[inline]
    pub fn new(l: f32, r: f32) -> Self {
        Frame { l, r }
    }

    /// Apply the same function to both channels (for mono-style effects that
    /// process each channel independently with their own state).
    #[inline]
    pub fn map<F: FnMut(f32) -> f32>(self, mut f: F) -> Self {
        Frame {
            l: f(self.l),
            r: f(self.r),
        }
    }
}

impl std::ops::Add for Frame {
    type Output = Frame;
    #[inline]
    fn add(self, rhs: Frame) -> Frame {
        Frame {
            l: self.l + rhs.l,
            r: self.r + rhs.r,
        }
    }
}

impl std::ops::AddAssign for Frame {
    #[inline]
    fn add_assign(&mut self, rhs: Frame) {
        self.l += rhs.l;
        self.r += rhs.r;
    }
}

impl std::ops::Mul<f32> for Frame {
    type Output = Frame;
    #[inline]
    fn mul(self, rhs: f32) -> Frame {
        Frame {
            l: self.l * rhs,
            r: self.r * rhs,
        }
    }
}

impl std::iter::Sum for Frame {
    fn sum<I: Iterator<Item = Frame>>(iter: I) -> Frame {
        iter.fold(Frame::SILENCE, |acc, f| acc + f)
    }
}
