use std::collections::HashMap;

use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{ AccountId, Balance, env };
use near_sdk::{ json_types::{U64, U128} };
use crate::types::{ WrappedBalance, WrappedDuration, Duration, Vote };
use crate::policy_item::{ PolicyItem };
use crate::proposal_status::{ ProposalStatus };

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct ProposalInput {
    pub description: String,
    pub kind: ProposalKind,
}

#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
#[serde(tag = "type")]
pub enum ProposalKind {
    NewCouncil { target: AccountId },
    RemoveCouncil { target: AccountId },
    Payout { target: AccountId, amount: WrappedBalance },
    ChangeVotePeriod { vote_period: WrappedDuration },
    ChangeBond { bond: WrappedBalance },
    ChangePolicy { policy: PolicyItem },
    ChangePurpose { purpose: String },
    ResoluteMarket { market_id: U64, payout_numerator: Option<Vec<U128>> },
    ChangeProtocolAddress { address: String },
    SetTokenWhitelist { whitelist: Vec<AccountId> },
    AddTokenWhitelist { to_add: AccountId },
    SetGov { new_gov: AccountId }
}

#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct Proposal {
    pub status: ProposalStatus,
    pub proposer: AccountId,
    pub description: String,
    pub kind: ProposalKind,
    pub last_vote: Duration,
    pub vote_period_end: Duration,
    pub vote_yes: u64,
    pub vote_no: u64,
    pub votes: HashMap<AccountId, Vote>,
}

impl Proposal {
    pub fn get_amount(&self) -> Option<Balance> {
        match &self.kind {
            ProposalKind::Payout { target,  amount } => Some(amount.0),
            _ => None,
        }
    }

    /// Compute new vote status given council size and current timestamp.
    pub fn vote_status(&self, policy: &PolicyItem, num_council: u64) -> ProposalStatus {
        let needed_votes = policy.num_votes(num_council);

        if self.vote_yes >= needed_votes {
            ProposalStatus::Success
        } else if env::block_timestamp() < self.vote_period_end {
            ProposalStatus::Vote
        } else {
            ProposalStatus::Reject
        }
    }
}
