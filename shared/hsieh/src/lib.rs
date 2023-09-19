// Paul Hsieh derivative license
//
// The derivative content includes raw computer source code, ideas, opinions,
// and excerpts whose original source is covered under another license and
// transformations of such derivatives. Note that mere excerpts by themselves
// (with the exception of raw source code) are not considered derivative works
// under this license. Use and redistribution is limited to the following
// conditions:
//
// One may not create a derivative work which, in any way, violates the Paul
// Hsieh exposition license described above on the original content.
// One may not apply a license to a derivative work that precludes anyone else
// from using and redistributing derivative content.
// One may not attribute any derivative content to authors not involved in the
// creation of the content, though an attribution to the author is not
// necessary.

//! Rust implementation of Peter Hsiehs hash function as presented on
//! <http://www.azillionmonkeys.com/qed/hash.html>
use core::num::Wrapping;

fn pop_word(data: &mut &[u8]) -> u32 {
    let word = u16::from_le_bytes([data[0], data[1]]);
    *data = &data[2..];
    word as u32
}

/// Generate the hash value for the given data.
pub fn hash(mut data: &[u8]) -> u32 {
    let len = data.len() as u32;

    if len == 0 {
        return 0;
    }

    let remainder = len & 3;
    let blocks = len >> 2;

    let mut hash = Wrapping(len);
    for _ in 0..blocks {
        hash += pop_word(&mut data);
        let tmp = Wrapping(pop_word(&mut data) << 11) ^ hash;
        hash = (hash << 16) ^ tmp;
        hash += hash >> 11;
    }

    match remainder {
        3 => {
            hash += pop_word(&mut data);
            hash ^= hash << 16;
            hash ^= (data[0] as u32) << 18;
            hash += hash >> 11;
        }
        2 => {
            hash += pop_word(&mut data);
            hash ^= hash << 11;
            hash += hash >> 17;
        }
        1 => {
            hash += data[0] as u32;
            hash ^= hash << 10;
            hash += hash >> 1;
        }
        0 => {
            // Nothing to do
        }
        _ => unreachable!(),
    }

    hash ^= hash << 3;
    hash += hash >> 5;
    hash ^= hash << 4;
    hash += hash >> 17;
    hash ^= hash << 25;
    hash += hash >> 6;

    hash.0
}

#[cfg(test)]
mod test {
    #[test]
    fn hash() {
        assert_eq!(super::hash(b"TestData"), 1397659898);
        assert_eq!(super::hash(b"Lorem Ipsum!"), 1190584371);
        assert_eq!(super::hash(b""), 0);
    }
}
