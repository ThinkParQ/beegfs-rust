//! BeeGFS compatible network message (de-)serialization

use anyhow::{bail, Result};
use bytes::{Buf, BufMut, BytesMut};
use std::collections::HashMap;
use std::hash::Hash;
use std::marker::PhantomData;
use std::mem::size_of;

pub trait Serializable {
    fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()>;
}

pub trait Deserializable {
    fn deserialize(des: &mut Deserializer<'_>) -> Result<Self>
    where
        Self: Sized;
}

/// Provides conversion functionality to and from BeeSerde serializable types.
///
/// Mainly meant for enums that need to be converted in to a raw integer type, which also might
/// differ between messages. The generic parameter allows implementing it for multiple types.
pub trait BeeSerdeConversion<S>: Sized {
    fn into_bee_serde(self) -> S;
    fn try_from_bee_serde(value: S) -> Result<Self>;
}

/// Interface for serialization helpers to be used with the `bee_serde` derive macro
///
/// Serialization helpers are meant to control the `bee_serde` macro in case a value in the
/// message struct shall be serialized as a different type or in case it doesn't have its own
/// [BeeSerde] implementation. Also necessary for maps and sequences since the serializer can't
/// know on its own whether to include collection size or not (it's totally message dependent).
///
/// # Example
///
/// ```ignore
/// #[derive(Debug, BeeSerde)]
/// pub struct ExampleMsg {
///     // Serializer doesn't know by itself whether or not C/C++ BeeGFS serializer expects sequence
///     // size included or not - in this case it is not
///     #[bee_serde(as = Seq<false, _>)]
///     int_sequence: Vec<u32>,
/// }
/// ```
pub trait BeeSerdeHelper<In> {
    fn serialize_as(data: &In, ser: &mut Serializer<'_>) -> Result<()>;
    fn deserialize_as(des: &mut Deserializer<'_>) -> Result<In>;
}

/// Serializes one BeeGFS message into a provided buffer
pub struct Serializer<'a> {
    /// The target buffer
    target_buf: &'a mut BytesMut,
    /// BeeGFS message feature flags obtained, used for conditional serialization by certain
    /// messages. To be set by the serialization function.
    pub msg_feature_flags: u16,
    /// The number of bytes written to the buffer
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
    /// Creates a new Serializer object
    ///
    /// `msg_feature_flags` can be accessed from the (de-)serialization definition and is used for
    /// conditional serialization on some messages.
    /// `msg_feature_flags` is supposed to be obtained from the message definition, and is used
    /// for conditional serialization by certain messages.
    pub fn new(target_buf: &'a mut BytesMut) -> Self {
        Self {
            target_buf,
            msg_feature_flags: 0,
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

    /// Serialize the given slice as bytes as expected by BeeGFS
    pub fn bytes(&mut self, v: &[u8]) -> Result<()> {
        self.target_buf.put(v);
        self.bytes_written += v.len();
        Ok(())
    }

    /// Serialize the given slice as c string (including terminating 0 byte) as expected by BeeGFS
    ///
    /// `align_to` optionally aligns the data as expected by BeeGFS deserializer. Must match the
    /// original message definition. Some BeeGFS messages / types use this (usually set to 4), some
    /// don't.
    pub fn cstr(&mut self, v: &[u8], align_to: usize) -> Result<()> {
        self.u32(v.len() as u32)?;
        self.bytes(v)?;
        self.u8(0)?;

        if align_to != 0 {
            // Total amount of bytes written for this CStr modulo align_to - results in the number
            // of pad bytes to add for alignment
            let padding = (v.len() + 4 + 1) % align_to;

            if padding != 0 {
                self.zeroes(align_to - padding)?;
            }
        }

        Ok(())
    }

    /// Serialize the elements in the given iterator into a sequence as expected by BeeGFS.
    ///
    /// "Sequence" is implemented for containers like `std::vector` and `std::list` and works the
    /// same for all.
    ///
    /// `include_total_size` determines whether the total size of the sequence shall be included in
    /// the serialized data. Must match the original message definition. Some BeeGFS messages /
    /// types use this, some don't.
    ///
    /// `f` expects a closure that handles serialization of all the elements in the sequence.
    pub fn seq<T>(
        &mut self,
        elements: impl IntoIterator<Item = T>,
        include_total_size: bool,
        f: impl Fn(&mut Self, T) -> Result<()>,
    ) -> Result<()> {
        let before = self.bytes_written;

        // For the total size and length of the sequence we insert placeholders to be replaced
        // later when the values are known
        //
        // On a side note, this is the sole reason why the Serialization struct needs a borrow to
        // `BytesMut` and not the generic `BufMut` - the latter doesn't allow random access to
        // already written data
        let size_pos = if include_total_size {
            let size_pos = self.bytes_written;
            self.u32(0xFFFFFFFFu32)?;
            size_pos
        } else {
            0
        };

        let count_pos = self.bytes_written;
        self.u32(0xFFFFFFFFu32)?;

        let mut count = 0u32;
        // Serialize each element of the sequence using the given closure
        for e in elements {
            count += 1;
            f(self, e)?;
        }

        // Now that the total amount and size of the serialized sequence elements is known, replace
        // the placeholders in the beginning of the sequence with the actual values

        if include_total_size {
            let written = (self.bytes_written - before) as u32;
            for (p, b) in written.to_le_bytes().iter().enumerate() {
                self.target_buf[size_pos + p] = *b;
            }
        }

        for (p, b) in count.to_le_bytes().iter().enumerate() {
            self.target_buf[count_pos + p] = *b;
        }

        Ok(())
    }

    /// Serialize the key value pairs in the given iterator into a map as expected by BeeGFS.
    ///
    /// "map" is implemented for maps like `std::map`.
    ///
    /// `include_total_size` determines whether the total size of the map shall be included in
    /// the serialized data. Must match the original message definition. Some BeeGFS messages /
    /// types use this, some don't.
    ///
    /// `f_key` and `f_value` expect closures that handles serialization of all the keys and values.
    pub fn map<K, V>(
        &mut self,
        elements: impl IntoIterator<Item = (K, V)>,
        include_total_size: bool,
        f_key: impl Fn(&mut Self, K) -> Result<()>,
        f_value: impl Fn(&mut Self, V) -> Result<()>,
    ) -> Result<()> {
        // A map is actually serialized like a sequence with each element containing the key
        // first and value second
        self.seq(elements, include_total_size, |s, (k, v)| {
            f_key(s, k)?;
            f_value(s, v)?;
            Ok(())
        })
    }

    /// Fills with `n` zeroes
    pub fn zeroes(&mut self, n: usize) -> Result<()> {
        for _ in 0..n {
            self.u8(0)?;
        }
        Ok(())
    }

    /// The amount of bytes written to the buffer (so far)
    pub fn bytes_written(&self) -> usize {
        self.bytes_written
    }
}

/// Deserializes one BeeGFS message from the given buffer
pub struct Deserializer<'a> {
    /// The source buffer
    source_buf: &'a [u8],
    /// BeeGFS message feature flags obtained from the message definition, used for
    /// conditional deserialization by certain messages.
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
    /// Creates a new Deserializer object
    ///
    /// `msg_feature_flags` is supposed to be obtained from the message definition, and is used
    /// for conditional serialization by certain messages.
    pub fn new(source_buf: &'a [u8], msg_feature_flags: u16) -> Self {
        Self {
            source_buf,
            msg_feature_flags,
        }
    }

    /// Checks that the whole buffer has been consumed - meant to be called after deserialization
    /// as a sanity check.
    pub fn finish(&self) -> Result<()> {
        let len = self.source_buf.len();
        if len > 0 {
            bail!("Did not consume the whole buffer, {len} bytes are left");
        }

        Ok(())
    }

    fn_deserialize_primitive!(u8, get_u8);
    fn_deserialize_primitive!(i8, get_i8);
    fn_deserialize_primitive!(u16, get_u16_le);
    fn_deserialize_primitive!(i16, get_i16_le);
    fn_deserialize_primitive!(u32, get_u32_le);
    fn_deserialize_primitive!(i32, get_i32_le);
    fn_deserialize_primitive!(u64, get_u64_le);
    fn_deserialize_primitive!(i64, get_i64_le);

    /// Deserialize a block of bytes as expected by BeeGFS
    pub fn bytes(&mut self, len: usize) -> Result<Vec<u8>> {
        let mut v = vec![0; len];

        self.check_remaining(len)?;
        self.source_buf.copy_to_slice(&mut v);

        Ok(v)
    }

    /// Deserialize a BeeGFS serialized c string
    ///
    /// `align_to` optionally aligns the data as expected by BeeGFS serializer. Must match the
    /// original message definition. Some BeeGFS messages / types use this (usually set to 4), some
    /// don't.
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
            // Total amount of bytes read for this CStr modulo align_to - results in the number
            // of pad bytes to skip due to alignment
            let padding = (v.len() + 4 + 1) % align_to;

            if padding != 0 {
                self.skip(align_to - padding)?;
            }
        }

        Ok(v)
    }

    /// Deserializes a BeeGFS serialized sequence of elements
    ///
    /// "Sequence" is implemented for containers like `std::vector` and `std::list` and works the
    /// same for all.
    ///
    /// `include_total_size` determines whether the total size of the sequence is included in
    /// the serialized data. Must match the original message definition. Some BeeGFS messages /
    /// types use this, some don't.
    ///
    /// `f` expects a closure that handles deserialization of all the elements in the sequence.
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

    /// Deserialized a BeeGFS serialized map
    ///
    /// "map" is implemented for maps like `std::map`.
    ///
    /// `include_total_size` determines whether the total size of the map is included in
    /// the serialized data. Must match the original message definition. Some BeeGFS messages /
    /// types use this, some don't.
    ///
    /// `f_key` and `f_value` expect closures that handles deserialization of all the keys and
    /// values.
    pub fn map<K: Hash + Eq, V>(
        &mut self,
        include_total_size: bool,
        f_key: impl Fn(&mut Self) -> Result<K>,
        f_value: impl Fn(&mut Self) -> Result<V>,
    ) -> Result<HashMap<K, V>> {
        // Unlike in serialization we do not forward deserialization to self.seq() to avoid double
        // allocation of Vec and Hashmap

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

    /// Skips `n` bytes
    ///
    /// The opposite of fill_zeroes() in serialization.
    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.check_remaining(n)?;
        self.source_buf.advance(n);

        Ok(())
    }

    /// Ensures that the source buffer has at least `n` bytes left
    ///
    /// Meant to check that there are enough bytes left before calling `Bytes` functions that would
    /// panic otherwise (which we wan't to avoid)
    fn check_remaining(&self, n: usize) -> Result<()> {
        if self.source_buf.remaining() < n {
            bail!(
                "Unexpected end of source buffer. Needed at least {}, got {}",
                n,
                self.source_buf.remaining()
            );
        }
        Ok(())
    }
}

/// Serialize an arbitrary type as Integer
///
/// Note: Can potentially be used for non-integers, but is not practical due to the [Copy]
/// requirement
pub struct Int<Out>(PhantomData<Out>);

impl<In, Out> BeeSerdeHelper<In> for Int<Out>
where
    In: BeeSerdeConversion<Out> + Copy,
    Out: Serializable + Deserializable,
{
    fn serialize_as(data: &In, ser: &mut Serializer<'_>) -> Result<()> {
        let o: Out = (*data).into_bee_serde();
        o.serialize(ser)
    }

    fn deserialize_as(des: &mut Deserializer<'_>) -> Result<In> {
        In::try_from_bee_serde(Out::deserialize(des)?)
    }
}

/// Serialize a `Vec<T>` as sequence
///
/// `INCLUDE_SIZE` controls the `include_total_size` parameter of `seq(...)`[Serializer::seq].
/// `T` must implement [BeeSerde].
pub struct Seq<const INCLUDE_SIZE: bool, T>(PhantomData<T>);

impl<const INCLUDE_SIZE: bool, T: Serializable + Deserializable> BeeSerdeHelper<Vec<T>>
    for Seq<INCLUDE_SIZE, T>
{
    fn serialize_as(data: &Vec<T>, ser: &mut Serializer<'_>) -> Result<()> {
        ser.seq(data.iter(), INCLUDE_SIZE, |ser, e| e.serialize(ser))
    }

    fn deserialize_as(des: &mut Deserializer<'_>) -> Result<Vec<T>> {
        des.seq(INCLUDE_SIZE, |des| T::deserialize(des))
    }
}

/// Serialize a `HashMap<K, V>` as map
///
/// `INCLUDE_SIZE` controls the `include_total_size` parameter of `seq(...)`.
/// `K` must implement [BeeSerde] and [Hash], `V` must implement [BeeSerde].
pub struct Map<const INCLUDE_SIZE: bool, K, V>(PhantomData<(K, V)>);

impl<
        const INCLUDE_SIZE: bool,
        K: Serializable + Deserializable + Eq + Hash,
        V: Serializable + Deserializable,
    > BeeSerdeHelper<HashMap<K, V>> for Map<INCLUDE_SIZE, K, V>
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

/// Serialize a slice of bytes as CStr
///
/// `ALIGN_TO` controls the `align_to` parameter of `cstr(...)`.
pub struct CStr<const ALIGN_TO: usize>;

impl<const ALIGN_TO: usize, Input> BeeSerdeHelper<Input> for CStr<ALIGN_TO>
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

// Implement BeeSerde for all integer primitives including conversion into bool
macro_rules! impl_traits_for_primitive {
    ($t:ident) => {
        impl Serializable for $t {
            fn serialize(&self, ser: &mut Serializer<'_>) -> Result<()> {
                ser.$t(*self)
            }
        }

        impl Deserializable for $t {
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn primitives() {
        let mut buf = BytesMut::new();

        let mut ser = Serializer::new(&mut buf);
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
        des.finish().unwrap();
    }

    #[test]
    fn bytes() {
        let bytes: Vec<u8> = vec![0, 1, 2, 3, 4, 5];

        let mut buf = BytesMut::new();

        let mut ser = Serializer::new(&mut buf);
        ser.bytes(&bytes).unwrap();
        ser.bytes(&bytes).unwrap();

        assert_eq!(12, ser.bytes_written);

        let mut des = Deserializer::new(&buf, 0);
        assert_eq!(bytes, des.bytes(6).unwrap());
        assert_eq!(bytes, des.bytes(6).unwrap());

        des.finish().unwrap();
    }

    #[test]
    fn cstr() {
        let str: Vec<u8> = "text".into();

        let mut buf = BytesMut::new();

        let mut ser = Serializer::new(&mut buf);
        ser.cstr(&str, 0).unwrap();
        ser.cstr(&str, 4).unwrap();
        ser.cstr(&str, 5).unwrap();

        assert_eq!(
            // alignment applies to string length + null byte terminator
            // Last one with align_to = 5 is intended and correct: Wrote 9 bytes, 9 % align_to = 1,
            // align_to - 1 = 4
            (4 + 4 + 1) + (4 + 4 + 1) + (4 + 4 + 1 + 4),
            ser.bytes_written
        );

        let mut des = Deserializer::new(&buf, 0);
        assert_eq!(str, des.cstr(0).unwrap());
        assert_eq!(str, des.cstr(4).unwrap());
        assert_eq!(str, des.cstr(5).unwrap());

        des.finish().unwrap();
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
            pub c2: HashMap<u16, Vec<Vec<u8>>>,
        }

        impl Serializable for S {
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
                    |ser, v| ser.seq(v.iter(), false, |ser, e| ser.cstr(e, 0)),
                )
                .unwrap();

                Ok(())
            }
        }

        impl Deserializable for S {
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
                        |des| des.seq(false, |des| des.cstr(0)),
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

        let mut ser = Serializer::new(&mut buf);

        s.serialize(&mut ser).unwrap();

        assert_eq!(
            1 + 8
                + (8 + 3 * 8)
                + (4 + 2 + 8)
                + (8 + (4 + 2 + 4))
                + (8 + (2 + (4 + (4 + 3 + 1) + (4 + 5 + 1)))),
            ser.bytes_written
        );

        let mut des = Deserializer::new(&buf, 0);

        let s2 = S::deserialize(&mut des).unwrap();

        assert_eq!(s, s2);
        des.finish().unwrap();
    }

    #[test]
    fn wrong_buffer_len() {
        let bytes: Vec<u8> = vec![0, 1, 2, 3, 4, 5];

        let mut buf = BytesMut::new();
        let mut ser = Serializer::new(&mut buf);
        ser.bytes(&bytes).unwrap();

        let mut des = Deserializer::new(&buf, 0);
        des.bytes(5).unwrap();

        // Some buffer left
        des.finish().unwrap_err();

        // Consume too much
        des.bytes(2).unwrap_err();

        des.bytes(1).unwrap();

        // Complete buffer consumed
        des.finish().unwrap();
    }
}
