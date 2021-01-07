use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::serde::{Deserialize, Serialize};

use near_sdk::json_types::{U64, U128};

/// Raw type for duration in nanoseconds
pub type Duration = u64;

/// Duration in nanosecond wrapped into a struct for JSON serialization as a string.
pub type WrappedDuration = U64;

/// Raw type for timestamp in nanoseconds
pub type Timestamp = u64;

/// Timestamp in nanosecond wrapped into a struct for JSON serialization as a string.
pub type WrappedTimestamp = U64;

/// Balance wrapped into a struct for JSON serialization as a string.
pub type WrappedBalance = U128;

#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub enum Vote {
    Yes,
    No,
}

#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Clone)]
#[serde(crate = "near_sdk::serde")]
#[serde(untagged)]
pub enum NumOrRatio {
    Number(u64),
    Ratio(u64, u64),
}