//! Query system user and group ids

use std::sync::OnceLock;
use tokio::sync::{Mutex, MutexGuard};

// SAFETY (applies to both user and group id iterators)
//
// * The global mutex assures that no more than one iterator object exists and therefore
// undefined results by concurrent access are prevented (it obviously doesn't prevent reusing
// libc::setpwent() elsewhere, don't do this!)
// * getpwent() / getgrent() return the next entry or a nullptr in case EOF is reached or an
// error occurs. Both cases are covered.

static MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

/// Iterator over system user IDs
pub struct UserIDIter<'a> {
    _lock: MutexGuard<'a, ()>,
}

/// Retrieves system user IDs.
///
/// Uses `getpwent()` libc call family.
///
/// # Return value
/// An iterator iterating over the systems user IDs. This function will block all other tasks
/// until the iterator is dropped.
pub async fn user_ids<'a>() -> UserIDIter<'a> {
    let _lock = MUTEX.get_or_init(|| Mutex::new(())).lock().await;

    // SAFETY: See above
    unsafe {
        libc::setpwent();
    }

    UserIDIter { _lock }
}

impl Drop for UserIDIter<'_> {
    fn drop(&mut self) {
        // SAFETY: See above
        unsafe {
            libc::endpwent();
        }
    }
}

impl Iterator for UserIDIter<'_> {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        // SAFETY: See above
        unsafe {
            let passwd: *mut libc::passwd = libc::getpwent();
            if passwd.is_null() {
                None
            } else {
                Some((*passwd).pw_uid)
            }
        }
    }
}

/// Iterator over system group IDs
pub struct GroupIDIter<'a> {
    _lock: MutexGuard<'a, ()>,
}

/// Retrieves system group IDs.
///
/// Uses `getgrent()` libc call.
///
/// # Return value
/// An iterator iterating over the systems group IDs. This function will block all other tasks
/// until the iterator is dropped.
pub async fn group_ids<'a>() -> GroupIDIter<'a> {
    let _lock = MUTEX.get_or_init(|| Mutex::new(())).lock().await;

    // SAFETY: See above
    unsafe {
        libc::setgrent();
    }

    GroupIDIter { _lock }
}

impl Drop for GroupIDIter<'_> {
    fn drop(&mut self) {
        // SAFETY: See above
        unsafe {
            libc::endgrent();
        }
    }
}

impl Iterator for GroupIDIter<'_> {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        // SAFETY: See above
        unsafe {
            let passwd: *mut libc::group = libc::getgrent();
            if passwd.is_null() {
                None
            } else {
                Some((*passwd).gr_gid)
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use itertools::Itertools;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn user_ids_thread_safety() {
        let tasks: Vec<_> = (0..16)
            .map(|_| tokio::spawn(async { user_ids().await.collect() }))
            .collect();

        let mut results = vec![];
        for t in tasks {
            let r: Vec<_> = t.await.unwrap();
            results.push(r);
        }

        assert!(results.into_iter().all_equal());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn group_ids_thread_safety() {
        let tasks: Vec<_> = (0..16)
            .map(|_| tokio::spawn(async { group_ids().await.collect() }))
            .collect();

        let mut results = vec![];
        for t in tasks {
            let r: Vec<_> = t.await.unwrap();
            results.push(r);
        }

        assert!(results.into_iter().all_equal());
    }
}
