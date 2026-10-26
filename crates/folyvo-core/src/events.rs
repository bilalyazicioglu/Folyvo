use crate::error::Result;
use crate::model::{ItemId, ShareEvent};
use crate::store::{repo, Store};

/// Everything that happened to an item's copies, oldest first.
pub fn share_events(store: &Store, item: &ItemId) -> Result<Vec<ShareEvent>> {
    repo::events_for_item(store.conn(), item)
}
