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
    /// Proposal is finalized
    Finalized,
    /// Proposal is rejected
    Rejected

}

impl ProposalStatus {
    pub fn is_finished(&self) -> bool {
        self == &ProposalStatus::Rejected || self == &ProposalStatus::Finalized
    }
}