use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::serde::{Deserialize, Serialize};

#[derive(BorshSerialize, BorshDeserialize, Eq, PartialEq, Debug, Serialize, Deserialize, Clone)]
#[serde(crate = "near_sdk::serde")]
pub enum ProposalStatus {
    /// Proposal is in active voting stage.
    Vote,
    /// Proposal has successfully passed.
    Success,
    /// Proposal was rejected by the vote.
    Reject,

}

impl ProposalStatus {
    pub fn is_finalized(&self) -> bool {
        self != &ProposalStatus::Vote
    }
}