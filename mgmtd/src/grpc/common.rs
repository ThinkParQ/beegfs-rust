pub(super) const QUOTA_NOT_ENABLED_STR: &str = "Quota support is not enabled";

// Fetching pages of 1M from quota_usage takes around 2100ms on my slow developer laptop (using a
// release build). In comparison, a page size of 100k takes around 750ms which is far worse. This
// feels like a good middle point to not let the requester wait too long and not waste too many db
// thread cycles with overhead.
pub(super) const QUOTA_STREAM_PAGE_LIMIT: usize = 1_000_000;

// Need to hit a compromise between memory footprint and speed. Bigger is better if multiple
// pages need to be fetched but doesn't matter too much if not. Each entry is roughly 50 -
// 60 bytes, so 100k (= 5-6MB) feels fine. And it is still big enough to give a significant
// boost to throughput for big numbers.
pub(super) const QUOTA_STREAM_BUF_SIZE: usize = 100_000;
