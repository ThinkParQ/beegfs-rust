use std::sync::atomic::{AtomicU32, Ordering};

/// Helper for tracking maximum ongoing concurrent operations
#[must_use]
pub struct PeakConcurrencyTracker<'a> {
    in_flight: &'a AtomicU32,
    peaked_at: Option<u32>,
}

impl<'a> PeakConcurrencyTracker<'a> {
    /// Creates a peak concurrency tracker object. Caller needs to define two static atomics passed
    /// in here that are used to determine how many instances of this object exist and store the
    /// highest value seen before.
    pub fn new(in_flight: &'a AtomicU32, peak: &'a AtomicU32) -> Self {
        let count = in_flight.fetch_add(1, Ordering::Relaxed) + 1;
        let peaked_at = if count > peak.fetch_max(count, Ordering::Relaxed) {
            Some(count)
        } else {
            None
        };

        Self {
            in_flight,
            peaked_at,
        }
    }

    /// If this instance exceeded a previous maximum, returns the new maximum value, otherwise
    /// `None`.
    pub fn peaked(&self) -> Option<u32> {
        self.peaked_at
    }
}

impl<'a> Drop for PeakConcurrencyTracker<'a> {
    fn drop(&mut self) {
        self.in_flight.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Creates a peak concurrency tracker object including the required atomics. Each expansion site
/// gets its own tracker / atomics.
#[macro_export]
macro_rules! peak_concurrency_tracker {
    () => {{
        static IN_FLIGHT: ::std::sync::atomic::AtomicU32 = ::std::sync::atomic::AtomicU32::new(0);
        static PEAK: ::std::sync::atomic::AtomicU32 = ::std::sync::atomic::AtomicU32::new(0);
        $crate::concurrency_tracker::PeakConcurrencyTracker::new(&IN_FLIGHT, &PEAK)
    }};
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn peak() {
        let in_flight = AtomicU32::new(0);
        let peak = AtomicU32::new(0);

        {
            let a = PeakConcurrencyTracker::new(&in_flight, &peak);
            assert_eq!(Some(1), a.peaked());

            let b = PeakConcurrencyTracker::new(&in_flight, &peak);
            assert_eq!(Some(2), b.peaked());

            assert_eq!(2, in_flight.load(Ordering::Relaxed));
        }

        // Dropping frees up the slots again, but the peak is remembered
        assert_eq!(0, in_flight.load(Ordering::Relaxed));
        assert_eq!(2, peak.load(Ordering::Relaxed));

        // Reaching the previous peak, but not exceeding it, doesn't report a new one
        let c = PeakConcurrencyTracker::new(&in_flight, &peak);
        assert_eq!(None, c.peaked());
        let d = PeakConcurrencyTracker::new(&in_flight, &peak);
        assert_eq!(None, d.peaked());

        // Exceeding it does
        let e = PeakConcurrencyTracker::new(&in_flight, &peak);
        assert_eq!(Some(3), e.peaked());
        assert_eq!(3, peak.load(Ordering::Relaxed));
    }
}
