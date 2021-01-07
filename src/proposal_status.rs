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
    /// Vote for proposal has failed due (not enuough votes).
    Fail,
    /// Given voting policy, the uncontested minimum of votes was acquired.
    /// Delaying the finalization of the proposal to check that there is no contenders (who would vote against).
    Delay,
}

impl ProposalStatus {
    pub fn is_finalized(&self) -> bool {
        self != &ProposalStatus::Vote && self != &ProposalStatus::Delay
    }
}