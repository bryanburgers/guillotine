use std::fmt::Display;

/// A structure that generates fresh futures
#[derive(Debug, Default)]
pub struct FutureIdGenerator {
    current: u64,
}

impl FutureIdGenerator {
    /// Generate a fresh, unique future ID
    pub fn fresh(&mut self) -> FutureId {
        let id = self.current;
        self.current += 1;
        FutureId::from_u64(id)
    }
}

/// A unique ID for a future
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FutureId(u64);

impl FutureId {
    /// Convert this ID into its internal u64 value.
    ///
    /// We need to do this so we can hand it to epoll.
    pub fn to_u64(&self) -> u64 {
        self.0
    }

    /// Convert a u64 value back to a FutureId.
    ///
    /// We need to do this because epoll only deals with u64s.
    pub fn from_u64(input: u64) -> Self {
        Self(input)
    }
}

impl Display for FutureId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
