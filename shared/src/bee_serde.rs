//! BeeGFS compatible network message (de-)serialization

use anyhow::{bail, Result};
use bytes::BytesMut;
pub use bytes::{Buf, BufMut};
use std::collections::HashMap;
use std::hash::Hash;
use std::marker::PhantomData;
use std::mem::size_of;

pub trait BeeSerde {
    // TODO no anyhow
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()>;
    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self>
    where
        Self: Sized;
}

pub struct Serializer<'a> {
    target_buf: &'a mut BytesMut,
    pub msg_feature_flags: u16,
    bytes_written: usize,
}

macro_rules! fn_serialize_primitive {
    ($P:ident, $put_f:ident) => {
        pub fn $P(&mut self, v: $P) -> Result<()> {
            self.target_buf.$put_f(v);
            self.bytes_written += size_of::<$P>();
            Ok(())
        }
    };
}

impl<'a> Serializer<'a> {
    pub fn new(target_buf: &'a mut BytesMut, msg_feature_flags: u16) -> Self {
        Self {
            target_buf,
            msg_feature_flags,
            bytes_written: 0,
        }
    }

    fn_serialize_primitive!(u8, put_u8);
    fn_serialize_primitive!(i8, put_i8);
    fn_serialize_primitive!(u16, put_u16_le);
    fn_serialize_primitive!(i16, put_i16_le);
    fn_serialize_primitive!(u32, put_u32_le);
    fn_serialize_primitive!(i32, put_i32_le);
    fn_serialize_primitive!(u64, put_u64_le);
    fn_serialize_primitive!(i64, put_i64_le);

    pub fn bytes(&mut self, v: &[u8]) -> Result<()> {
        self.target_buf.put(v);
        self.bytes_written += v.len();
        Ok(())
    }

    pub fn cstr(&mut self, v: &[u8], align_to: usize) -> Result<()> {
        self.u32(v.len() as u32)?;
        self.bytes(v)?;
        self.u8(0)?;

        if align_to != 0 {
            let padding = (v.len() + 1) % align_to;
            if padding != 0 {
                self.zeroes(align_to - padding)?;
            }
        }

        Ok(())
    }

    pub fn string(&mut self, v: &str) -> Result<()> {
        let b = v.as_bytes();

        self.u32(b.len() as u32)?;
        self.bytes(b)?;

        Ok(())
    }

    pub fn seq<T>(
        &mut self,
        v: impl IntoIterator<Item = T>,
        include_total_size: bool,
        f: impl Fn(&mut Self, T) -> Result<()>,
    ) -> Result<()> {
        let before = self.bytes_written;

        let size_pos = if include_total_size {
            let size_pos = self.bytes_written;
            // placeholder for the total size in bytes
            self.u32(0xFFFFFFFFu32)?;
            size_pos
        } else {
            0
        };

        let count_pos = self.bytes_written;
        // placeholder for the total length
        self.u32(0xFFFFFFFFu32)?;

        let mut count = 0u32;
        for e in v {
            count += 1;
            f(self, e)?;
        }

        if include_total_size {
            // overwrite the placeholder with the actual size of the collection
            let written = (self.bytes_written - before) as u32;
            for (p, b) in written.to_le_bytes().iter().enumerate() {
                self.target_buf[size_pos + p] = *b;
            }
        }

        // overwrite the placeholder with the actual collection count
        for (p, b) in count.to_le_bytes().iter().enumerate() {
            self.target_buf[count_pos + p] = *b;
        }

        Ok(())
    }

    pub fn map<K, V>(
        &mut self,
        iter: impl IntoIterator<Item = (K, V)>,
        include_total_size: bool,
        f_key: impl Fn(&mut Self, K) -> Result<()>,
        f_value: impl Fn(&mut Self, V) -> Result<()>,
    ) -> Result<()> {
        self.seq(iter, include_total_size, |s, (k, v)| {
            f_key(s, k)?;
            f_value(s, v)?;
            Ok(())
        })
    }

    pub fn zeroes(&mut self, n: usize) -> Result<()> {
        for _ in 0..n {
            self.u8(0)?;
        }
        Ok(())
    }

    pub fn bytes_written(&self) -> usize {
        self.bytes_written
    }
}

pub struct Deserializer<'a> {
    source_buf: &'a [u8],
    pub msg_feature_flags: u16,
}

macro_rules! fn_deserialize_primitive {
    ($P:ident, $get_f:ident) => {
        pub fn $P(&mut self) -> Result<$P> {
            self.check_remaining(size_of::<$P>())?;
            Ok(self.source_buf.$get_f())
        }
    };
}

impl<'a> Deserializer<'a> {
    pub fn new(source_buf: &'a [u8], msg_feature_flags: u16) -> Self {
        Self {
            source_buf,
            msg_feature_flags,
        }
    }

    fn_deserialize_primitive!(u8, get_u8);
    fn_deserialize_primitive!(i8, get_i8);
    fn_deserialize_primitive!(u16, get_u16_le);
    fn_deserialize_primitive!(i16, get_i16_le);
    fn_deserialize_primitive!(u32, get_u32_le);
    fn_deserialize_primitive!(i32, get_i32_le);
    fn_deserialize_primitive!(u64, get_u64_le);
    fn_deserialize_primitive!(i64, get_i64_le);

    pub fn bool_from<S: IntoBool + BeeSerde>(&mut self) -> Result<bool> {
        Ok(S::deserialize(self)?.into_bool())
    }

    pub fn bytes(&mut self, len: usize) -> Result<Vec<u8>> {
        let mut v = vec![0; len];

        self.check_remaining(len)?;
        self.source_buf.copy_to_slice(&mut v);

        Ok(v)
    }

    pub fn cstr(&mut self, align_to: usize) -> Result<Vec<u8>> {
        let len = self.u32()? as usize;

        let mut v = vec![0; len];

        self.check_remaining(len)?;
        self.source_buf.copy_to_slice(&mut v);

        let terminator: u8 = self.u8()?;
        if terminator != 0 {
            bail!("Invalid CStr terminator {terminator}");
        }

        if align_to != 0 {
            let padding = (v.len() + 1) % align_to;

            if padding != 0 {
                self.skip(align_to - padding)?;
            }
        }

        Ok(v)
    }

    pub fn string(&mut self) -> Result<String> {
        let len = self.u32()? as usize;

        let mut bytes = vec![0; len];
        self.source_buf.copy_to_slice(&mut bytes);

        Ok(String::from_utf8(bytes)?)
    }

    pub fn seq<T>(
        &mut self,
        include_total_size: bool,
        f: impl Fn(&mut Self) -> Result<T>,
    ) -> Result<Vec<T>> {
        if include_total_size {
            self.skip(size_of::<u32>())?;
        }

        let len = self.u32()? as usize;

        let mut v = Vec::with_capacity(len);
        for _ in 0..len {
            v.push(f(self)?);
        }

        Ok(v)
    }

    pub fn map<K: Hash + Eq, V>(
        &mut self,
        include_total_size: bool,
        f_key: impl Fn(&mut Self) -> Result<K>,
        f_value: impl Fn(&mut Self) -> Result<V>,
    ) -> Result<HashMap<K, V>> {
        if include_total_size {
            self.skip(size_of::<u32>())?;
        }

        let len = self.u32()? as usize;

        let mut v = HashMap::with_capacity(len);
        for _ in 0..len {
            v.insert(f_key(self)?, f_value(self)?);
        }

        Ok(v)
    }

    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.check_remaining(n)?;
        self.source_buf.advance(n);

        Ok(())
    }

    fn check_remaining(&self, len: usize) -> Result<()> {
        if self.source_buf.remaining() < len {
            bail!(
                "Unexpected end of source buffer. Needed at least {}, got {}",
                len,
                self.source_buf.remaining()
            );
        }
        Ok(())
    }
}

pub trait BeeSerdeAs<Input> {
    fn serialize_as(data: &Input, ser: &mut Serializer<'_>) -> Result<()>;
    fn deserialize_as(des: &mut Deserializer<'_>) -> Result<Input>;
}

pub struct Int<Output>(PhantomData<Output>);

impl<Input, Target> BeeSerdeAs<Input> for Int<Target>
where
    Input: TryInto<Target> + Copy,
    Target: TryInto<Input> + BeeSerde,
    anyhow::Error: From<<Input as TryInto<Target>>::Error> + From<Target::Error>,
{
    fn serialize_as(data: &Input, ser: &mut Serializer<'_>) -> Result<()> {
        let o: Target = (*data).try_into()?;
        o.serialize(ser)
    }

    fn deserialize_as(des: &mut Deserializer<'_>) -> Result<Input> {
        Ok(Target::deserialize(des)?.try_into()?)
    }
}

pub struct BoolAsInt<Output>(PhantomData<Output>);

impl<Target> BeeSerdeAs<bool> for BoolAsInt<Target>
where
    bool: TryInto<Target> + Copy,
    Target: IntoBool + BeeSerde,
    anyhow::Error: From<<bool as TryInto<Target>>::Error>,
{
    fn serialize_as(data: &bool, ser: &mut Serializer<'_>) -> Result<()> {
        let o: Target = (*data).try_into()?;
        o.serialize(ser)
    }

    fn deserialize_as(des: &mut Deserializer<'_>) -> Result<bool> {
        Ok(Target::deserialize(des)?.into_bool())
    }
}

pub struct Seq<const INCLUDE_SIZE: bool, T>(PhantomData<T>);

impl<const INCLUDE_SIZE: bool, T: BeeSerde> BeeSerdeAs<Vec<T>> for Seq<INCLUDE_SIZE, T> {
    fn serialize_as(data: &Vec<T>, ser: &mut Serializer<'_>) -> Result<()> {
        ser.seq(data.iter(), INCLUDE_SIZE, |ser, e| e.serialize(ser))
    }

    fn deserialize_as(des: &mut Deserializer<'_>) -> Result<Vec<T>> {
        des.seq(INCLUDE_SIZE, |des| T::deserialize(des))
    }
}

pub struct Map<const INCLUDE_SIZE: bool, K, V>(PhantomData<(K, V)>);

impl<const INCLUDE_SIZE: bool, K: BeeSerde + Eq + Hash, V: BeeSerde> BeeSerdeAs<HashMap<K, V>>
    for Map<INCLUDE_SIZE, K, V>
{
    fn serialize_as(data: &HashMap<K, V>, ser: &mut Serializer<'_>) -> Result<()> {
        ser.map(
            data.iter(),
            INCLUDE_SIZE,
            |ser, k| k.serialize(ser),
            |ser, v| v.serialize(ser),
        )
    }

    fn deserialize_as(des: &mut Deserializer<'_>) -> Result<HashMap<K, V>> {
        des.map(
            INCLUDE_SIZE,
            |des| K::deserialize(des),
            |des| V::deserialize(des),
        )
    }
}

pub struct CStr<const ALIGN_TO: usize>;

impl<const ALIGN_TO: usize, Input> BeeSerdeAs<Input> for CStr<ALIGN_TO>
where
    Input: AsRef<[u8]>,
    Vec<u8>: TryInto<Input>,
    anyhow::Error: From<<Vec<u8> as TryInto<Input>>::Error>,
{
    fn serialize_as(data: &Input, ser: &mut Serializer<'_>) -> Result<()> {
        ser.cstr(data.as_ref(), ALIGN_TO)
    }

    fn deserialize_as(des: &mut Deserializer<'_>) -> Result<Input> {
        Ok(des.cstr(ALIGN_TO)?.try_into()?)
    }
}

pub trait IntoBool: seal_into_bool::Sealed {
    fn into_bool(self) -> bool;
}

mod seal_into_bool {
    pub trait Sealed {}
}

macro_rules! impl_traits_for_primitive {
    ($t:ident) => {
        impl seal_into_bool::Sealed for $t {}
        impl IntoBool for $t {
            fn into_bool(self) -> bool {
                match self {
                    0 => false,
                    _ => true,
                }
            }
        }

        impl BeeSerde for $t {
            fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
                ser.$t(*self)
            }

            fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
                des.$t()
            }
        }
    };
}

impl_traits_for_primitive!(u8);
impl_traits_for_primitive!(i8);
impl_traits_for_primitive!(u16);
impl_traits_for_primitive!(i16);
impl_traits_for_primitive!(u32);
impl_traits_for_primitive!(i32);
impl_traits_for_primitive!(u64);
impl_traits_for_primitive!(i64);

impl BeeSerde for String {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
        ser.string(self)
    }

    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self> {
        des.string()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn primitives() {
        let mut buf = BytesMut::new();

        let mut ser = Serializer::new(&mut buf, 0);
        ser.u8(123).unwrap();
        ser.i8(-123).unwrap();
        ser.u16(22222).unwrap();
        ser.i16(-22222).unwrap();
        ser.u32(0x11223344).unwrap();
        ser.i32(-0x11223344).unwrap();
        ser.u64(0xAABBCCDDEEFF1122u64).unwrap();
        ser.i64(-0x1ABBCCDDEEFF1122i64).unwrap();

        // 1 + 2 + 2 + 4 + 4 + 8
        assert_eq!(1 + 1 + 2 + 2 + 4 + 4 + 8 + 8, ser.bytes_written);

        let mut des = Deserializer::new(&buf, 0);
        assert_eq!(123, des.u8().unwrap());
        assert_eq!(-123, des.i8().unwrap());
        assert_eq!(22222, des.u16().unwrap());
        assert_eq!(-22222, des.i16().unwrap());
        assert_eq!(0x11223344, des.u32().unwrap());
        assert_eq!(-0x11223344, des.i32().unwrap());
        assert_eq!(0xAABBCCDDEEFF1122, des.u64().unwrap());
        assert_eq!(-0x1ABBCCDDEEFF1122, des.i64().unwrap());

        assert!(des.u64().is_err());

        assert_eq!(0, des.source_buf.remaining());
    }

    #[test]
    fn bytes() {
        let bytes: Vec<u8> = vec![0, 1, 2, 3, 4, 5];

        let mut buf = BytesMut::new();

        let mut ser = Serializer::new(&mut buf, 0);
        ser.bytes(&bytes).unwrap();
        ser.bytes(&bytes).unwrap();

        assert_eq!(12, ser.bytes_written);

        let mut des = Deserializer::new(&buf, 0);
        assert_eq!(bytes, des.bytes(6).unwrap());
        assert_eq!(bytes, des.bytes(6).unwrap());

        assert_eq!(0, des.source_buf.remaining());
    }

    #[test]
    fn cstr() {
        let str: Vec<u8> = "text".into();

        let mut buf = BytesMut::new();

        let mut ser = Serializer::new(&mut buf, 0);
        ser.cstr(&str, 0).unwrap();
        ser.cstr(&str, 4).unwrap();
        ser.cstr(&str, 5).unwrap();

        assert_eq!(
            // alignment applies to string length + null byte terminator
            (4 + 4 + 1) + (4 + 4 + 1 + 3) + (4 + 4 + 1),
            ser.bytes_written
        );

        let mut des = Deserializer::new(&buf, 0);
        assert_eq!(str, des.cstr(0).unwrap());
        assert_eq!(str, des.cstr(4).unwrap());
        assert_eq!(str, des.cstr(5).unwrap());

        assert_eq!(0, des.source_buf.remaining());
    }

    #[test]
    fn string() {
        let mut buf = BytesMut::new();

        let mut ser = Serializer::new(&mut buf, 0);
        ser.string("one string").unwrap();
        ser.string("another string").unwrap();

        assert_eq!((4 + 10) + (4 + 14), ser.bytes_written);

        let mut des = Deserializer::new(&buf, 0);
        assert_eq!("one string", des.string().unwrap());
        assert_eq!("another string", des.string().unwrap());

        assert_eq!(0, des.source_buf.remaining());
    }

    #[test]
    fn nested() {
        #[derive(Clone, PartialEq, Eq, Debug)]
        struct S {
            pub var_u8: u8,
            pub var_u64: u64,
            pub v: Vec<u64>,
            pub m: HashMap<u16, i64>,
            pub c: Vec<HashMap<i16, i32>>,
            pub c2: HashMap<u16, Vec<String>>,
        }

        impl BeeSerde for S {
            fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
                ser.u8(self.var_u8).unwrap();
                ser.u64(self.var_u64).unwrap();
                ser.seq(self.v.iter(), true, |ser, e| e.serialize(ser))
                    .unwrap();

                ser.map(
                    self.m.iter(),
                    false,
                    |ser, k| ser.u16(*k),
                    |ser, v| ser.i64(*v),
                )
                .unwrap();

                ser.seq(self.c.iter(), true, |ser, e| {
                    ser.map(
                        e.iter(),
                        false,
                        |ser, k| k.serialize(ser),
                        |ser, v| v.serialize(ser),
                    )
                })
                .unwrap();

                ser.map(
                    self.c2.iter(),
                    true,
                    |ser, k| k.serialize(ser),
                    |ser, v| ser.seq(v.iter(), false, |ser, e| ser.string(e)),
                )
                .unwrap();

                Ok(())
            }

            fn deserialize(des: &mut Deserializer<'_>) -> Result<Self>
            where
                Self: Sized,
            {
                Ok(S {
                    var_u8: des.u8()?,
                    var_u64: des.u64()?,
                    v: des.seq(true, |des| des.u64())?,
                    m: des.map(false, |des| des.u16(), |des| des.i64())?,
                    c: des.seq(true, |des| des.map(false, |des| des.i16(), |des| des.i32()))?,
                    c2: des.map(
                        true,
                        |des| des.u16(),
                        |des| des.seq(false, |des| des.string()),
                    )?,
                })
            }
        }

        let s = S {
            var_u8: 200,
            var_u64: 3000000,
            v: vec![23424354, 111, 9999],
            m: HashMap::from([(300, -300)]),
            c: vec![HashMap::from([(-1000, -12345)])],
            c2: HashMap::from([(18, vec!["aaa".into(), "bbbbb".into()])]),
        };

        let mut buf = BytesMut::new();

        let mut ser = Serializer::new(&mut buf, 0);

        s.serialize(&mut ser).unwrap();

        assert_eq!(
            1 + 8
                + (8 + 3 * 8)
                + (4 + 2 + 8)
                + (8 + (4 + 2 + 4))
                + (8 + (2 + (4 + (4 + 3) + (4 + 5)))),
            ser.bytes_written
        );

        let mut des = Deserializer::new(&buf, 0);

        let s2 = S::deserialize(&mut des).unwrap();

        assert_eq!(s, s2);
    }
}
