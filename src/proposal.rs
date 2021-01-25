use std::collections::HashMap;

use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::{ AccountId, Balance, env };
use near_sdk::{ json_types::{U64, U128} };
use crate::types::{ WrappedBalance, WrappedDuration, Duration, Vote };
use crate::policy_item::{ PolicyItem };
use crate::proposal_status::{ ProposalStatus };
use crate::utils::{ vote_requirement };

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct ProposalInput {
    pub target: AccountId,
    pub description: String,
    pub kind: ProposalKind,
}

#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
#[serde(tag = "type")]
pub enum ProposalKind {
    NewCouncil,
    RemoveCouncil,
    Payout { amount: WrappedBalance },
    ChangeVotePeriod { vote_period: WrappedDuration },
    ChangeBond { bond: WrappedBalance },
    ChangePolicy { policy: Vec<PolicyItem> },
    ChangePurpose { purpose: String },
    ResoluteMarket { market_id: U64, payout_numerator: Option<Vec<U128>> },
    ChangeProtocolAddress { address: String },
    SetTokenWhitelist { whitelist: Vec<AccountId> },
    AddTokenWhitelist { to_add: AccountId }
}

#[derive(BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct Proposal {
    pub status: ProposalStatus,
    pub proposer: AccountId,
    pub target: AccountId,
    pub description: String,
    pub kind: ProposalKind,
    pub vote_period_end: Duration,
    pub vote_yes: u64,
    pub vote_no: u64,
    pub votes: HashMap<AccountId, Vote>,
}

impl Proposal {
    pub fn get_amount(&self) -> Option<Balance> {
        match self.kind {
            ProposalKind::Payout { amount } => Some(amount.0),
            _ => None,
        }
    }

    /// Compute new vote status given council size and current timestamp.
    pub fn vote_status(&self, policy: &[PolicyItem], num_council: u64) -> ProposalStatus {
        let votes_required = vote_requirement(policy, num_council, self.get_amount());
        let max_votes = policy[policy.len() - 1].num_votes(num_council);

        if self.vote_yes >= max_votes {
            ProposalStatus::Success
        } else if self.vote_yes >= votes_required && self.vote_no == 0 {
            if env::block_timestamp() > self.vote_period_end {
                ProposalStatus::Success
            } else {
                ProposalStatus::Delay
            }
        } else if self.vote_no >= max_votes {
            ProposalStatus::Reject
        } else if env::block_timestamp() > self.vote_period_end || self.vote_yes + self.vote_no == num_council {
            ProposalStatus::Fail
        } else {
            ProposalStatus::Vote
        }
    }
}
