use std::collections::HashMap;

// use near_lib::types::{Duration, WrappedBalance, WrappedDuration};
use near_sdk::{ AccountId, Balance, env, near_bindgen, Promise};
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{UnorderedSet, Vector};
use crate::utils::{ to_yocto };
use crate::types::{ NumOrRatio, Vote };

mod proposal_status;
mod proposal;
mod policy_item;
mod types;
mod utils;
mod tests;

use policy_item::{ PolicyItem };
use proposal::{ Proposal, ProposalInput, ProposalKind };
use proposal_status::{ ProposalStatus };
use types::{ Duration, WrappedBalance, WrappedDuration };

#[global_allocator]
static ALLOC: near_sdk::wee_alloc::WeeAlloc<'_> = near_sdk::wee_alloc::WeeAlloc::INIT;

const MAX_DESCRIPTION_LENGTH: usize = 280;
const MINIMAL_NEAR_FOR_COUNCIL: u128 = 5000;

#[near_bindgen]
#[derive(BorshSerialize, BorshDeserialize)]
pub struct FluxDAO {
    purpose: String,
    bond: Balance,
    vote_period: Duration,
    grace_period: Duration,
    policy: Vec<PolicyItem>,
    council: UnorderedSet<AccountId>,
    proposals: Vector<Proposal>,
}

impl Default for FluxDAO {
    fn default() -> Self {
        env::panic(b"FluxDAO should be initialized before usage")
    }
}

#[near_bindgen]
impl FluxDAO {
    #[init]
    pub fn new(
        purpose: String,
        council: Vec<AccountId>,
        bond: WrappedBalance,
        vote_period: WrappedDuration,
        grace_period: WrappedDuration,
    ) -> Self {
        assert!(!env::state_exists(), "The contract is already initialized");
        let mut dao = Self {
            purpose,
            bond: bond.into(),
            vote_period: vote_period.into(),
            grace_period: grace_period.into(),
            policy: vec![PolicyItem {
                max_amount: 0.into(),
                votes: NumOrRatio::Ratio(1, 2),
            }],
            council: UnorderedSet::new(b"c".to_vec()),
            proposals: Vector::new(b"p".to_vec()),
        };
        for account_id in council {
            dao.council.insert(&account_id);
        }
        dao
    }

    #[payable]
    pub fn add_proposal(&mut self, proposal: ProposalInput) -> u64 {
        // TOOD: add also extra storage cost for the proposal itself.

        assert!(
            proposal.description.len() < MAX_DESCRIPTION_LENGTH,
            "Description length is too long"
        );

        // Input verification.
        match proposal.kind {
            ProposalKind::NewCouncil => {
                assert!(env::attached_deposit() >= to_yocto(MINIMAL_NEAR_FOR_COUNCIL), "Not enough deposit");
            }

            ProposalKind::ChangePolicy { ref policy } => {
                assert!(env::attached_deposit() >= self.bond, "Not enough deposit");

                for i in 1..policy.len() {
                    assert!(
                        policy[i].max_amount.0 > policy[i - 1].max_amount.0,
                        "Policy must be sorted, item {} is wrong",
                        i
                    );
                }
            }

            _ => {
                assert!(env::attached_deposit() >= self.bond, "Not enough deposit");
            }
        }

        let p = Proposal {
            status: ProposalStatus::Vote,
            proposer: env::predecessor_account_id(),
            target: proposal.target,
            description: proposal.description,
            kind: proposal.kind,
            vote_period_end: env::block_timestamp() + self.vote_period,
            vote_yes: 0,
            vote_no: 0,
            votes: HashMap::default(),
        };

        self.proposals.push(&p);
        self.proposals.len() - 1
    }

    pub fn get_vote_period(&self) -> WrappedDuration {
        self.vote_period.into()
    }

    pub fn get_bond(&self) -> WrappedBalance {
        self.bond.into()
    }

    pub fn get_council(&self) -> Vec<AccountId> {
        self.council.to_vec()
    }

    pub fn get_num_proposals(&self) -> u64 {
        self.proposals.len()
    }

    pub fn get_proposals(&self, from_index: u64, limit: u64) -> Vec<Proposal> {
        (from_index..std::cmp::min(from_index + limit, self.proposals.len()))
            .map(|index| self.proposals.get(index).unwrap())
            .collect()
    }

    pub fn get_proposal(&self, id: u64) -> Proposal {
        self.proposals.get(id).expect("Proposal not found")
    }

    pub fn get_purpose(&self) -> String {
        self.purpose.clone()
    }

    pub fn vote(&mut self, id: u64, vote: Vote) {
        assert!(
            self.council.contains(&env::predecessor_account_id()),
            "Only council can vote"
        );
        let mut proposal = self.proposals.get(id).expect("No proposal with such id");
        assert_eq!(
            proposal.status,
            ProposalStatus::Vote,
            "Proposal already finalized"
        );
        if proposal.vote_period_end < env::block_timestamp() {
            env::log(b"Voting period expired, finalizing the proposal");
            self.finalize(id);
            return;
        }
        assert!(
            !proposal.votes.contains_key(&env::predecessor_account_id()),
            "Already voted"
        );
        match vote {
            Vote::Yes => proposal.vote_yes += 1,
            Vote::No => proposal.vote_no += 1,
        }
        proposal.votes.insert(env::predecessor_account_id(), vote);
        let post_status = proposal.vote_status(&self.policy, self.council.len());
        // If just changed from vote to Delay, adjust the expiration date to grace period.
        if !post_status.is_finalized() {
            proposal.vote_period_end = env::block_timestamp() + self.grace_period;
            proposal.status = post_status.clone();
        }
        self.proposals.replace(id, &proposal);
        // Finalize if this vote is done.
        if post_status.is_finalized() {
            self.finalize(id);
        }
    }

    // TODO: Add function to exit dao

    pub fn finalize(&mut self, id: u64) {
        let mut proposal = self.proposals.get(id).expect("No proposal with such id");
        assert!(
            !proposal.status.is_finalized(),
            "Proposal already finalized"
        );
        proposal.status = proposal.vote_status(&self.policy, self.council.len());
        match proposal.status {
            ProposalStatus::Success => {
                env::log(b"Vote succeeded");
                let target = proposal.target.clone();
                Promise::new(proposal.proposer.clone()).transfer(self.bond);
                match proposal.kind {
                    ProposalKind::NewCouncil => {
                        self.council.insert(&target);
                    }
                    ProposalKind::RemoveCouncil => {
                        // TODO: Give stake back
                        self.council.remove(&target);
                    }
                    ProposalKind::Payout { amount } => {
                        Promise::new(target).transfer(amount.0);
                    }
                    ProposalKind::ChangeVotePeriod { vote_period } => {
                        self.vote_period = vote_period.into();
                    }
                    ProposalKind::ChangeBond { bond } => {
                        self.bond = bond.into();
                    }
                    ProposalKind::ChangePolicy{ ref policy } => {
                        self.policy = policy.clone();
                    }
                    ProposalKind::ChangePurpose{ ref purpose } => {
                        self.purpose = purpose.clone();
                    }
                };
            }
            ProposalStatus::Reject => {
                env::log(b"Proposal rejected");
            }
            ProposalStatus::Fail => {
                // If no majority vote, let's return the bond.
                env::log(b"Proposal vote failed");
                Promise::new(proposal.proposer.clone()).transfer(self.bond);
            }
            ProposalStatus::Vote | ProposalStatus::Delay => {
                env::panic(b"voting period has not expired and no majority vote yet")
            }
        }
        self.proposals.replace(id, &proposal);
    }
}
