use crate::source::Source;
use crate::*;
pub use cache_input::CacheInput;
use std::collections::HashMap;
use std::marker::PhantomData;
use tokio::sync::watch::{self, Ref};

#[derive(Clone, Debug)]
pub struct Cache<C> {
    rx: watch::Receiver<CacheMap>,
    _config: PhantomData<C>,
}

impl<C: Config> Cache<C> {
    pub fn get<K: Field<BelongsTo = C>>(&self) -> K::Value {
        let borrow = self.rx.borrow();
        let value = borrow.get(K::KEY).unwrap_or_else(|| {
            panic!(
                "Should never happen: Config key not found in map: {}",
                K::KEY
            )
        });

        value
            .as_ref()
            .as_any()
            .downcast_ref::<K::Value>()
            .unwrap_or_else(|| {
                panic!(
                    "Should never happen: Invalid type for downcast of {}",
                    K::KEY
                )
            })
            .clone()
    }

    pub fn borrow_all(&self) -> Ref<CacheMap> {
        self.rx.borrow()
    }
}

pub async fn from_source<C: Config>(
    source: impl Source,
) -> Result<(CacheInput<C>, Cache<C>), ConfigError> {
    let (tx, rx) = watch::channel(HashMap::new());

    Ok((
        CacheInput::from_source(source, tx).await?,
        Cache {
            rx,
            _config: PhantomData,
        },
    ))
}

#[cfg(test)]
mod test {
    use super::*;
    use async_trait::async_trait;
    use std::ops::Range;

    define_config!(
        struct DummyConfig,
        FieldU8: u8 = 100,
        FieldString: String = "default".into(),
        FieldComplex: Option<Range<u8>> = Some(0..4),
    );

    struct DummySource;

    #[async_trait]
    impl Source for DummySource {
        async fn get(&self) -> Result<ConfigMap, BoxedError> {
            DummyConfig::default_map().map_err(|err| Box::new(err) as BoxedError)
        }
    }

    #[tokio::test]
    async fn set_get() {
        let (mut cache_input, cache) = from_source::<DummyConfig>(DummySource {}).await.unwrap();

        assert_eq!(100, cache.get::<FieldU8>());

        assert_eq!("default", cache.get::<FieldString>());
        cache_input
            .set_raw(
                [(
                    FieldString::KEY.to_owned(),
                    FieldString::serialize(&"new_value".to_string()).unwrap(),
                )]
                .into(),
            )
            .await
            .unwrap();
        assert_eq!("new_value", cache.get::<FieldString>());

        assert_eq!(Some(0..4), cache.get::<FieldComplex>());
    }

    #[tokio::test]
    async fn invalid_input() {
        let (mut cache_input, cache) = from_source::<DummyConfig>(DummySource {}).await.unwrap();

        // check for invalid key
        cache_input
            .set_raw([("InvalidKey".to_string(), "123".to_string())].into())
            .await
            .unwrap_err();

        // check for invalid JSON
        cache_input
            .set_raw([("FieldString".to_string(), "}invalid json".to_string())].into())
            .await
            .unwrap_err();

        assert_eq!("default", cache.get::<FieldString>());
    }
}
