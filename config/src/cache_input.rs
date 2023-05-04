use crate::source::Source;
use crate::*;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::sync::watch::{self};

#[derive(Clone, Debug)]
pub struct CacheInput<C> {
    tx: Arc<watch::Sender<CacheMap>>,
    _config: PhantomData<C>,
}

impl<C: Config> CacheInput<C> {
    pub(crate) async fn from_source(
        source: impl Source,
        tx: watch::Sender<CacheMap>,
    ) -> Result<Self, ConfigError> {
        let initial_map = source.get().await?;
        C::check_map_completeness(&initial_map)?;

        let mut this = CacheInput {
            tx: Arc::new(tx),
            _config: PhantomData,
        };

        this.set_raw(initial_map).await?;

        Ok(this)
    }

    pub async fn set_raw(&mut self, entries: ConfigMap) -> Result<(), ConfigError> {
        let mut any_map = HashMap::with_capacity(entries.len());

        for (k, v) in entries.into_iter() {
            if !C::ALL_KEYS.contains(&k.as_str()) {
                return Err(ConfigError::UndefinedKey(k));
            }

            let any_value = C::deserialize_to_any(&k, &v)?;
            any_map.insert(k, any_value);
        }

        self.tx.send_modify(|m| {
            for (k, v) in any_map.into_iter() {
                m.insert(k, v);
            }
        });

        Ok(())
    }
}
