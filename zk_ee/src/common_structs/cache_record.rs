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
    #[inline(always)]
    pub fn new_empty() -> Self {
        Self {
            value: None,
            metadata: Default::default(),
        }
    }

    #[inline(always)]
    pub fn new(value: V) -> Self {
        Self {
            value: Some(value),
            metadata: Default::default(),
        }
    }
}

impl<V, M> CacheRecord<V, M> {
    #[inline(always)]
    pub fn new_empty_with_metadata(metadata: M) -> Self {
        Self {
            value: None,
            metadata,
        }
    }

    #[inline(always)]
    pub fn value(&self) -> Option<&V> {
        self.value.as_ref()
    }

    #[inline(always)]
    pub fn materialized_value(&self) -> Result<&V, InternalError> {
        self.value
            .as_ref()
            .ok_or_else(|| internal_error!("Cache record value must be materialized"))
    }

    #[inline(always)]
    pub fn materialize(&mut self, value: V) {
        self.value = Some(value);
    }

    #[inline(always)]
    pub fn metadata(&self) -> &M {
        &self.metadata
    }

    #[must_use]
    /// Updates value and metadata using callback
    #[inline(always)]
    pub fn update<F>(&mut self, f: F) -> Result<(), InternalError>
    where
        F: FnOnce(&mut Option<V>, &mut M) -> Result<(), InternalError>,
    {
        f(&mut self.value, &mut self.metadata)
    }

    #[must_use]
    /// Updates a materialized value and metadata using callback
    #[inline(always)]
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
    #[inline(always)]
    pub fn update_metadata<F>(&mut self, f: F) -> Result<(), SystemError>
    where
        F: FnOnce(&mut M) -> Result<(), SystemError>,
    {
        f(&mut self.metadata)
    }

    /// Updates the metadata with an infallible callback.
    #[inline(always)]
    pub fn update_metadata_infallible<F>(&mut self, f: F)
    where
        F: FnOnce(&mut M),
    {
        f(&mut self.metadata)
    }
}

#[cfg(test)]
mod tests {
    use super::CacheRecord;

    #[test]
    fn constructors_and_value_access_match_materialization_state() {
        let empty = CacheRecord::<u32, u32>::new_empty();
        assert!(empty.value().is_none());

        let present = CacheRecord::<u32, u32>::new(11);
        assert_eq!(present.value(), Some(&11));
        assert_eq!(present.materialized_value().unwrap(), &11);
    }

    #[test]
    fn update_helpers_work_for_empty_and_materialized_records() {
        let mut empty = CacheRecord::<u32, u32>::new_empty_with_metadata(3);

        empty
            .update(|value, metadata| {
                assert!(value.is_none());
                *value = Some(7);
                *metadata = 5;
                Ok(())
            })
            .unwrap();
        assert_eq!(empty.value(), Some(&7));
        assert_eq!(empty.metadata(), &5);

        empty
            .update_metadata(|metadata| {
                *metadata = 9;
                Ok(())
            })
            .unwrap();
        assert_eq!(empty.metadata(), &9);

        let mut present = CacheRecord::<u32, u32>::new(13);
        present
            .update(|value, metadata| {
                let value = value.as_mut().unwrap();
                *value = 17;
                *metadata = 19;
                Ok(())
            })
            .unwrap();
        assert_eq!(present.value(), Some(&17));
        assert_eq!(present.metadata(), &19);
    }

    #[test]
    fn empty_record_materializes_in_place() {
        let mut record = CacheRecord::<u32, u32>::new_empty_with_metadata(7);

        assert!(record.value().is_none());
        assert!(record.materialized_value().is_err());
        assert!(record
            .update_materialized(|_, _| Ok::<_, crate::system::errors::internal::InternalError>(()))
            .is_err());

        record.materialize(11);

        assert_eq!(record.materialized_value().unwrap(), &11);

        record
            .update_materialized(|value, metadata| {
                *value = 13;
                *metadata = 17;
                Ok(())
            })
            .unwrap();

        assert_eq!(record.value(), Some(&13));
        assert_eq!(record.metadata(), &17);
    }
}
