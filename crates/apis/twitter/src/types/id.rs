use serde::{Deserialize, Serialize};
use serde_with::{DeserializeFromStr, SerializeDisplay};
use smartstring::alias::String as SmartString;

use crate::define_ids;

define_ids!(
    TweetId,
    PollId,
    UserId,
    RuleId,
    ContextAnnotationDomainId,
    ContextAnnotationEntityId
);

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PlaceId(pub SmartString);
