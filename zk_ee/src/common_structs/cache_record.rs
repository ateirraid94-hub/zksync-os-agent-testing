//! Wraps values with additional metadata used by IO caches

use crate::internal_error;
use crate::system::errors::internal::InternalError;
use crate::system::errors::system::SystemError;

#[derive(Clone, Default)]
/// A cache entry. Wraps actual value with some metadata used by caches.
pub struct CacheRecord<V, M> {
    value: Option<V>,
    metadata: M,
}

impl<V, M: Default> CacheRecord<V, M> {
    pub fn new_empty() -> Self {
        Self {
            value: None,
            metadata: Default::default(),
        }
    }

    pub fn new(value: V) -> Self {
        Self {
            value: Some(value),
            metadata: Default::default(),
        }
    }
}

impl<V, M> CacheRecord<V, M> {
    pub fn new_empty_with_metadata(metadata: M) -> Self {
        Self {
            value: None,
            metadata,
        }
    }

    pub fn value(&self) -> Option<&V> {
        self.value.as_ref()
    }

    pub fn materialized_value(&self) -> Result<&V, InternalError> {
        self.value
            .as_ref()
            .ok_or_else(|| internal_error!("Cache record value must be materialized"))
    }

    pub fn materialize(&mut self, value: V) {
        self.value = Some(value);
    }

    pub fn metadata(&self) -> &M {
        &self.metadata
    }

    #[must_use]
    /// Updates value and metadata using callback
    pub fn update<F>(&mut self, f: F) -> Result<(), InternalError>
    where
        F: FnOnce(&mut Option<V>, &mut M) -> Result<(), InternalError>,
    {
        f(&mut self.value, &mut self.metadata)
    }

    #[must_use]
    /// Updates a materialized value and metadata using callback
    pub fn update_materialized<F>(&mut self, f: F) -> Result<(), InternalError>
    where
        F: FnOnce(&mut V, &mut M) -> Result<(), InternalError>,
    {
        let value = self
            .value
            .as_mut()
            .ok_or_else(|| internal_error!("Cache record value must be materialized"))?;

        f(value, &mut self.metadata)
    }

    #[must_use]
    /// Updates the metadata
    pub fn update_metadata<F>(&mut self, f: F) -> Result<(), SystemError>
    where
        F: FnOnce(&mut M) -> Result<(), SystemError>,
    {
        f(&mut self.metadata)
    }

    /// Updates the metadata with an infallible callback.
    pub fn update_metadata_infallible<F>(&mut self, f: F)
    where
        F: FnOnce(&mut M),
    {
        f(&mut self.metadata)
    }
}
