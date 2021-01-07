use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::serde::{Deserialize, Serialize};

use crate::types::{ WrappedBalance, NumOrRatio };

/// Policy item, defining how many votes required to approve up to this much amount.
#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize, Clone)]
#[serde(crate = "near_sdk::serde")]
pub struct PolicyItem {
    pub max_amount: WrappedBalance,
    pub votes: NumOrRatio,
}

impl PolicyItem {
    pub fn num_votes(&self, num_council: u64) -> u64 {
        match self.votes {
            NumOrRatio::Number(num_votes) => num_votes,
            NumOrRatio::Ratio(l, r) => std::cmp::min(num_council * l / r + 1, num_council),
        }
    }
}