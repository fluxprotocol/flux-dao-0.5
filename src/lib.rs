use std::collections::HashMap;

// use near_lib::types::{Duration, WrappedBalance, WrappedDuration};
use near_sdk::{ ext_contract, AccountId, Balance, Gas, env, near_bindgen, Promise, PromiseOrValue};
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{UnorderedSet, Vector, UnorderedMap};
use near_sdk::json_types::{U64, U128};

// TODO: rewrite to same type of imports as from l19, if possible
use crate::utils::{ to_yocto };
pub use crate::types::{ NumOrRatio, Vote };

mod proposal_status;
mod proposal;
mod policy_item;
mod types;
mod utils;

use policy_item::{ PolicyItem };
pub use proposal::{ Proposal, ProposalInput, ProposalKind };
use proposal_status::{ ProposalStatus };
use types::{ Duration, WrappedBalance, WrappedDuration };

#[global_allocator]
static ALLOC: near_sdk::wee_alloc::WeeAlloc<'_> = near_sdk::wee_alloc::WeeAlloc::INIT;

const MAX_DESCRIPTION_LENGTH: usize = 280;
const MINIMAL_NEAR_FOR_COUNCIL: u128 = 1000;
const RESOLUTION_GAS: u64 = 5_000_000_000;


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
    last_voted: UnorderedMap<AccountId, u64>,
    protocol_address: AccountId
}

impl Default for FluxDAO {
    fn default() -> Self {
        env::panic(b"FluxDAO should be initialized before usage")
    }
}

#[ext_contract]
pub trait FluxProtocol {
    fn resolute_market(&mut self, market_id: U64, payout_numerator: Option<Vec<U128>>);
    fn set_token_whitelist(&mut self, whitelist: Vec<AccountId>);
    fn add_to_token_whitelist(&mut self, to_add: AccountId);
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
        protocol_address: String
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
            last_voted: UnorderedMap::new(b"e".to_vec()),
            protocol_address
        };
        for account_id in council.clone() {
            dao.council.insert(&account_id);
        }
        assert!(
            env::account_balance() >= council.len() as u128 * to_yocto(MINIMAL_NEAR_FOR_COUNCIL),
            "ERR_INSUFFICIENT_FUNDS"
        );
        dao
    }

    #[payable]
    pub fn add_proposal(&mut self, proposal: ProposalInput) -> U64 {
        // TODO: add also extra storage cost for the proposal itself.
        // TODO: transfer `env::attached_deposit() - to_yocto(MINIMAL_NEAR_FOR_COUNCIL)` back to env::predecessor_account
        assert!(
            proposal.description.len() < MAX_DESCRIPTION_LENGTH,
            "Description length is too long"
        );
        assert!(
            self.council.contains(&env::predecessor_account_id()),
            "Only council can create proposals"
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
        U64(self.proposals.len() - 1)
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

    pub fn get_num_proposals(&self) -> U64 {
        U64(self.proposals.len())
    }

    pub fn get_proposals(&self, from_index: U64, limit: U64) -> Vec<Proposal> {
        let from_index_u:u64 = from_index.into();
        let limit_u:u64 = limit.into();
        (from_index_u..std::cmp::min(from_index_u + limit_u, self.proposals.len()))
            .map(|index| self.proposals.get(index).unwrap())
            .collect()
    }

    pub fn get_proposal(&self, id: U64) -> Proposal {
        self.proposals.get(id.into()).expect("Proposal not found")
    }

    pub fn get_purpose(&self) -> String {
        self.purpose.clone()
    }

    pub fn vote(&mut self, id: U64, vote: Vote) {
        assert!(
            self.council.contains(&env::predecessor_account_id()),
            "Only council can vote"
        );
        let mut proposal = self.proposals.get(id.into()).expect("No proposal with such id");
        assert_eq!(
            proposal.status,
            ProposalStatus::Vote,
            "Proposal already finalized"
        );
        if proposal.vote_period_end < env::block_timestamp() {
            // env::log(b"Voting period expired, finalizing the proposal");
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
        self.last_voted.insert(&env::predecessor_account_id(), &id.into());

        let post_status = proposal.vote_status(&self.policy, self.council.len());
        // If just changed from vote to Delay, adjust the expiration date to grace period.
        // TODO validate this in a test
        // TODO validate ProposalStatus::Delay
        // TODO proposal for storage costs / returns
        if !post_status.is_finalized() {
            proposal.vote_period_end = env::block_timestamp() + self.grace_period;
            proposal.status = post_status.clone();
        }
        self.proposals.replace(id.into(), &proposal);
        // Finalize if this vote is done.
        if post_status.is_finalized() {
            self.finalize(id);
        }
    }

    pub fn finalize(&mut self, id: U64) {
        let mut proposal = self.proposals.get(id.into()).expect("No proposal with such id");
        assert!(
            !proposal.status.is_finalized(),
            "Proposal already finalized"
        );
        proposal.status = proposal.vote_status(&self.policy, self.council.len());
        match proposal.status {
            ProposalStatus::Success => {
                // env::log(b"Vote succeeded");
                let target = proposal.target.clone();
                Promise::new(proposal.proposer.clone()).transfer(self.bond);
                match proposal.kind {
                    ProposalKind::NewCouncil => {
                        self.council.insert(&target);
                    }
                    ProposalKind::RemoveCouncil => {
                        self.kick_user(&target);
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
                    ProposalKind::ResoluteMarket{ ref market_id, ref payout_numerator } => {
                        flux_protocol::resolute_market(
                            *market_id,
                            payout_numerator.clone(),
                            &self.protocol_address,
                            0,
                            RESOLUTION_GAS,
                        );
                    },
                    ProposalKind::ChangeProtocolAddress{ ref address } => {
                        self.protocol_address = address.to_string();
                    },
                    ProposalKind::SetTokenWhitelist{ ref whitelist } => {
                        flux_protocol::set_token_whitelist(
                            whitelist.clone(),
                            &self.protocol_address,
                            0,
                            RESOLUTION_GAS,
                        );
                    },
                    ProposalKind::AddTokenWhitelist{ ref to_add } => {
                        flux_protocol::add_to_token_whitelist(
                            to_add.clone(),
                            &self.protocol_address,
                            0,
                            RESOLUTION_GAS,
                        );
                    }
                };
            }
            ProposalStatus::Reject => {
                // env::log(b"Proposal rejected");
            }
            ProposalStatus::Fail => {
                // If no majority vote, let's return the bond.
                // env::log(b"Proposal vote failed");
                Promise::new(proposal.proposer.clone()).transfer(self.bond);
            }
            ProposalStatus::Vote | ProposalStatus::Delay => {
                env::panic(b"voting period has not expired and no majority vote yet")
            }
        }
        self.proposals.replace(id.into(), &proposal);
    }

    pub fn exit_dao(&mut self) {
        self.kick_user(&env::predecessor_account_id());
    }

    fn kick_user(&mut self, account_id: &AccountId) {
        let proposalid = self.last_voted.get(account_id);
        if !proposalid.is_none() {
            let proposal = self.proposals.get(proposalid.unwrap()).expect("ERR_PROPOSAL_NOT_FOUND");

            match proposal.kind {
                ProposalKind::RemoveCouncil => {
                    if &proposal.target != account_id {
                        assert!(proposal.status != ProposalStatus::Vote, "ERR_VOTING_ACTIVE");
                    }
                },
                _ => {
                    assert!(proposal.status != ProposalStatus::Vote, "ERR_VOTING_ACTIVE");
                }
            }
        }
        assert!(self.council.remove(account_id), "ERR_NOT_IN_COUNCIL");
        Promise::new(account_id.to_string()).transfer(to_yocto(MINIMAL_NEAR_FOR_COUNCIL));
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(test)]
mod tests {
    use near_sdk::MockedBlockchain;
    use near_sdk::{testing_env, VMContext};

    use super::*;

    fn alice() -> AccountId {
        "alice.near".to_string()
    }
    fn bob() -> AccountId {
        "bob.near".to_string()
    }
    fn carol() -> AccountId {
        "carol.near".to_string()
    }

    fn protocol_address() -> AccountId {
        "protocol".to_string()
    }

    fn get_context(predecessor_account_id: AccountId) -> VMContext {
        VMContext {
            current_account_id: alice(),
            signer_account_id: bob(),
            signer_account_pk: vec![0, 1, 2],
            predecessor_account_id,
            input: vec![],
            block_index: 0,
            block_timestamp: 0,
            account_balance: 0,
            account_locked_balance: 0,
            storage_usage: 10u64.pow(6),
            attached_deposit: 0,
            prepaid_gas: 10u64.pow(18),
            random_seed: vec![0, 1, 2],
            is_view: false,
            output_data_receivers: vec![],
            epoch_height: 0,
        }
    }

    fn init() -> FluxDAO {
        let mut dao = FluxDAO::new(
            String::from("do cool shit"),
            vec![alice()],
            U128(0),
            U64(0),
            U64(0),
            protocol_address()
        );
        dao
    }

    fn add_bob(contract : &mut FluxDAO) {
        let proposal = ProposalInput {
            target: bob(),
            description:  String::from("add bob"),
            kind: ProposalKind::NewCouncil,
        };
        let index:U64 = contract.add_proposal(proposal);
        contract.vote(index, Vote::Yes);
    }

    fn add_carol(contract : &mut FluxDAO) {
        let proposal = ProposalInput {
            target: carol(),
            description:  String::from("add carol"),
            kind: ProposalKind::NewCouncil,
        };
        let index:U64 = contract.add_proposal(proposal);
        contract.vote(index, Vote::Yes);

        let mut context = get_context(bob());
        testing_env!(context);
        contract.vote(index, Vote::Yes);
    }

    #[test]
    #[should_panic(expected = "ERR_INSUFFICIENT_FUNDS")]
    fn test_new_not_enough() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(800);
        testing_env!(context);
        let mut contract = init();
    }

    #[test]
    fn test_new() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);
        let mut contract = FluxDAO::new(
            String::from("do cool shit"),
            vec![alice(), bob()],
            U128(1_000_000_u128),
            U64(1000_u64),
            U64(2000_u64),
            protocol_address()
        );

        let purpose = String::from("do cool shit");
        let bond_amount:WrappedBalance  = U128(1_000_000_u128);
        let vote_period:WrappedDuration = U64(1000_u64);
        let grace_period:WrappedDuration = U64(2000_u64);
        let council = vec![alice(), bob()];
        assert_eq!(contract.get_bond(), bond_amount);
        assert_eq!(contract.get_vote_period(), vote_period);
        assert_eq!(contract.get_council(), council);
        assert_eq!(contract.get_num_proposals(), U64(0));
        assert_eq!(contract.get_purpose(), purpose);

        assert_eq!(contract.purpose, purpose);
        assert_eq!(contract.bond, bond_amount.into());
        //assert_eq!(contract.vote_period, vote_period.into());
        //assert_eq!(contract.grace_period, grace_period.into());
        assert_eq!(contract.policy.len(), 1);
        assert_eq!(contract.council.len(), 2);
        assert_eq!(contract.proposals.len(), 0);
    }

    #[test]
    #[should_panic(expected = "Not enough deposit")]
    fn test_add_new_council_proposal_insufficient_deposit() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(2000);
        testing_env!(context);

        let mut contract = FluxDAO::new(
            String::from("do cool shit"),
            vec![alice()],
            U128(0),
            U64(0),
            U64(0),
            protocol_address()
        );
        let proposal = ProposalInput {
            target: carol(),
            description: String::from("carol is cool"),
            kind: ProposalKind::NewCouncil,
        };
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(1);
        testing_env!(context);
        contract.add_proposal(proposal);
    }

    #[test]
    #[should_panic(expected = "Description length is too long")]
    fn test_add_new_council_invalid_description() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);

        let mut contract = init();
        let proposal = ProposalInput {
            target: carol(),
            description: String::from("a").repeat(281),
            kind: ProposalKind::NewCouncil,
        };
        contract.add_proposal(proposal);
    }

    #[test]
    fn test_add_new_council_proposal() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);

        let mut contract = init();
        let description = String::from("carol is cool");
        let proposal = ProposalInput {
            target: carol(),
            description: description.clone(),
            kind: ProposalKind::NewCouncil,
        };

        // Carol (not in council) creates a proposal to include her in the counsil
        let index:U64 = contract.add_proposal(proposal);
        // TODO, verify contract balance in NEAR
        assert_eq!(index, U64(0));
        assert_eq!(contract.get_num_proposals(), U64(1));
        let mut proposal = contract.get_proposal(U64(0));
        assert_eq!(proposal.status, ProposalStatus::Vote);
        assert_eq!(proposal.proposer, alice());
        assert_eq!(proposal.target, carol());
        assert_eq!(proposal.description, description);
        //assert_eq!(proposal.kind, ProposalKind::NewCouncil);
        // TODO, how to get block timestamp of tx
        //assert_eq!(proposal.vote_period_end, timestamp+U64(1000_u64));
        assert_eq!(proposal.vote_yes, 0);
        assert_eq!(proposal.vote_no, 0);

        context = get_context(alice());
        testing_env!(context);

        assert_eq!(contract.council.len(), 1);
        contract.vote(U64(0), Vote::Yes);
        proposal = contract.get_proposal(U64(0));
        assert_eq!(proposal.vote_yes, 1);
        assert_eq!(proposal.vote_no, 0);
        assert_eq!(proposal.status, ProposalStatus::Success);
        assert_eq!(proposal.proposer, alice());
        assert_eq!(proposal.target, carol());
        assert_eq!(proposal.description, description);
        assert_eq!(contract.council.len(), 2);
    }

    #[test]
    fn test_exit_dao() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);

        let mut contract = init();
        assert_eq!(contract.council.len(), 1);
        contract.exit_dao();
        assert_eq!(contract.council.len(), 0);
        // TODO test for running polls
    }

    #[test]
    #[should_panic(expected = "Only council can create proposals")]
    fn test_proposal_outside_council() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);

        let mut contract = init();
        let mut context = get_context(bob());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);

        let description = String::from("bob sucks");
        let proposal = ProposalInput {
            target: bob(),
            description: description.clone(),
            kind: ProposalKind::NewCouncil,
        };
        let index:U64 = contract.add_proposal(proposal);
        assert_eq!(index, U64(0));
    }

    #[test]
    fn test_remove_council_proposal() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);
        let mut contract = init();
        add_bob(&mut contract);
        add_carol(&mut contract);
        let description = String::from("bob sucks");
        let proposal = ProposalInput {
            target: bob(),
            description: description.clone(),
            kind: ProposalKind::RemoveCouncil,
        };
        let index:U64 = contract.add_proposal(proposal);

        assert_eq!(contract.council.len(), 3);

        let mut context = get_context(alice());
        testing_env!(context);
        contract.vote(index, Vote::Yes);

        let mut context = get_context(carol());
        // TODO, is sending near expected in this case
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);
        contract.vote(index, Vote::Yes);

        assert_eq!(contract.council.len(), 2);
    }

    #[test]
    fn test_remove_council_proposal_voteself() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);
        let mut contract = init();
        add_bob(&mut contract);
        add_carol(&mut contract);
        let description = String::from("bob sucks");
        let proposal = ProposalInput {
            target: bob(),
            description: description.clone(),
            kind: ProposalKind::RemoveCouncil,
        };
        let index:U64 = contract.add_proposal(proposal);
        assert_eq!(contract.council.len(), 3);

        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);
        contract.vote(index, Vote::Yes);

        let mut context = get_context(bob());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);
        contract.vote(index, Vote::Yes);

        assert_eq!(contract.council.len(), 2);
    }

    #[test]
    fn test_payout_proposal() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);

        let mut contract = init();
        let description = String::from("bob payout");
        let proposal = ProposalInput {
            target: bob(),
            description: description.clone(),
            kind: ProposalKind::Payout{ amount: U128(to_yocto(1)) },
        };
        contract.add_proposal(proposal);
        contract.vote(U64(0), Vote::Yes);
        // TODO, check balance
    }

    #[test]
    fn test_vote_period_proposal() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);

        let mut contract = init();
        let description = String::from("vote period");
        let proposal = ProposalInput {
            target: bob(),
            description: description.clone(),
            kind: ProposalKind::ChangeVotePeriod{ vote_period: U64(1) },
        };
        contract.add_proposal(proposal);
        assert_eq!(contract.get_vote_period(), U64(0));
        contract.vote(U64(0), Vote::Yes);
        assert_eq!(contract.get_vote_period(), U64(1));
        // TODO, check balance
    }

    #[test]
    fn test_change_bond_proposal() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);

        let mut contract = init();
        let description = String::from("bond");
        let proposal = ProposalInput {
            target: bob(),
            description: description.clone(),
            kind: ProposalKind::ChangeBond{ bond: U128(1) },
        };
        contract.add_proposal(proposal);
        assert_eq!(contract.get_bond(), U128(0));
        contract.vote(U64(0), Vote::Yes);
        assert_eq!(contract.get_bond(), U128(1));
        // TODO, check balance
    }

    #[test]
    fn test_change_policy_proposal() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);

        let mut contract = init();
        let description = String::from("policy");
        let policy = vec![PolicyItem {
            max_amount: 0.into(),
            votes: NumOrRatio::Ratio(1, 2),
        },
        PolicyItem {
            max_amount: 1.into(),
            votes: NumOrRatio::Ratio(1, 2),
        }];
        let proposal = ProposalInput {
            target: bob(),
            description: description.clone(),
            kind: ProposalKind::ChangePolicy{ policy },
        };
        contract.add_proposal(proposal);
        assert_eq!(contract.policy.len(), 1);
        contract.vote(U64(0), Vote::Yes);
        assert_eq!(contract.policy.len(), 2);
    }

    #[test]
    fn test_change_purpose_proposal() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);

        let purpose = String::from("do cool shit");
        let mut contract = init();
        let description = String::from("do cooler shit");
        let proposal = ProposalInput {
            target: bob(),
            description: description.clone(),
            kind: ProposalKind::ChangePurpose{ purpose: description.clone() },
        };
        contract.add_proposal(proposal);
        assert_eq!(contract.purpose, purpose);
        contract.vote(U64(0), Vote::Yes);
        assert_eq!(contract.purpose, description);
    }

    #[test]
    fn test_change_bond_proposal_fail() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);

        let mut contract = init();
        let description = String::from("bond");
        let proposal = ProposalInput {
            target: bob(),
            description: description.clone(),
            kind: ProposalKind::ChangeBond{ bond: U128(1) },
        };
        let index:U64 = contract.add_proposal(proposal);

        assert_eq!(contract.get_bond(), U128(0));
        contract.vote(index, Vote::No);
        assert_eq!(contract.get_bond(), U128(0));

        let p:Proposal = contract.get_proposal(index);
        assert_eq!(p.status, ProposalStatus::Reject);
        // TODO, check balance
    }

    #[test]
    fn test_vote__timestamp_fail() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);

        let purpose = String::from("do cool shit");
        let mut contract =  FluxDAO::new(
            String::from("do cool shit"),
            vec![alice()],
            U128(0),
            U64(100),
            U64(0),
            protocol_address()
        );
        add_bob(&mut contract);
        let proposal = ProposalInput {
            target: bob(),
            description: String::from("do cooler shit"),
            kind: ProposalKind::ChangePurpose{ purpose: String::from("do cooler shit") },
        };
        let index:U64 = contract.add_proposal(proposal);

        let mut context = get_context(alice());
        context.block_timestamp = 101;
        testing_env!(context);
        contract.finalize(index);

        let p:Proposal = contract.get_proposal(index);
        assert_eq!(p.status, ProposalStatus::Fail);
    }

    //TODO
    // #[test]
    // fn test_vote_delay() {
    //     let mut context = get_context(alice());
    //     context.attached_deposit = to_yocto(5000);
    //     testing_env!(context);

    //     let mut contract = init();
    //     let proposal = ProposalInput {
    //         target: bob(),
    //         description: String::from("policy"),
    //         kind: ProposalKind::ChangePolicy{ policy: vec![
    //             PolicyItem {
    //                 max_amount: 0.into(),
    //                 votes: NumOrRatio::Ratio(1, 3),
    //             }
    //         ]},
    //     };
    //     contract.add_proposal(proposal);
    //     contract.vote(U64(0), Vote::Yes);
    //     assert_eq!(1, 2);
    // }

    #[test]
    #[should_panic(expected = "Only council can vote")]
    fn test_no_council_vote() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);

        let mut contract = init();
        let proposal = ProposalInput {
            target: bob(),
            description: String::from("x"),
            kind: ProposalKind::ChangePurpose{ purpose:String::from("y") },
        };
        contract.add_proposal(proposal);

        let mut context = get_context(bob());
        testing_env!(context);
        contract.vote(U64(0), Vote::Yes);
    }

    #[test]
    #[should_panic(expected = "No proposal with such id")]
    fn test_no_proposal_vote() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);

        let mut contract = init();
        contract.vote(U64(0), Vote::Yes);
    }

    #[test]
    #[should_panic(expected = "Proposal already finalized")]
    fn test_proposal_already_finalized() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);

        let mut contract = init();
        let proposal = ProposalInput {
            target: bob(),
            description: String::from("x"),
            kind: ProposalKind::ChangePurpose{ purpose:String::from("y") },
        };
        contract.add_proposal(proposal);
        contract.vote(U64(0), Vote::Yes);
        contract.vote(U64(0), Vote::Yes);
    }

    #[test]
    #[should_panic(expected = "Already voted")]
    fn test_already_voted() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);

        let mut contract = init();
        add_bob(&mut contract);
        let proposal = ProposalInput {
            target: bob(),
            description: String::from("x"),
            kind: ProposalKind::ChangePurpose{ purpose:String::from("y") },
        };
        contract.add_proposal(proposal);
        contract.vote(U64(1), Vote::Yes);
        contract.vote(U64(1), Vote::Yes);
    }

    #[test]
    fn test_change_protocol_address() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);

        let protocol_new : AccountId = "protocol".to_string();
        let mut contract = init();
        assert_eq!(contract.protocol_address, protocol_address());
        let proposal = ProposalInput {
            target: bob(),
            description: String::from("change protocol address"),
            kind: ProposalKind::ChangeProtocolAddress{ address: protocol_new.clone() }
        };
        contract.add_proposal(proposal);
        contract.vote(U64(0), Vote::Yes);
        assert_eq!(contract.protocol_address, protocol_new.clone());
    }
}