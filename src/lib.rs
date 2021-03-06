use std::collections::HashMap;

// use near_lib::types::{Duration, WrappedBalance, WrappedDuration};
use near_sdk::{ ext_contract, AccountId, Balance, Gas, env, near_bindgen, Promise, PromiseOrValue, PromiseResult};
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
pub use proposal_status::{ ProposalStatus };
use types::{ Duration, WrappedBalance, WrappedDuration };

#[global_allocator]
static ALLOC: near_sdk::wee_alloc::WeeAlloc<'_> = near_sdk::wee_alloc::WeeAlloc::INIT;

const MAX_DESCRIPTION_LENGTH: usize = 280;
const RESOLUTION_GAS: u64 = 5_000_000_000_000;

const RESOLUTE_POLICY : PolicyItem = PolicyItem {
    max_amount: U128(0),
    votes: NumOrRatio::Number(4),
};

#[near_bindgen]
#[derive(BorshSerialize, BorshDeserialize)]
pub struct FluxDAO {
    purpose: String,
    bond: Balance,
    vote_period: Duration,
    grace_period: Duration,
    policy: PolicyItem,
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
    fn set_gov(&mut self, new_gov: AccountId);
    fn pause(&mut self);
    fn unpause(&mut self);

}

#[ext_contract(ext_self)]
pub trait ResolutionResolver {
    fn ft_resolve_protocol_call(
        &mut self,
        id: U64
    ) -> Promise;
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
            policy: PolicyItem {
                max_amount: 0.into(),
                votes: NumOrRatio::Ratio(1, 2),
            },
            council: UnorderedSet::new(b"c".to_vec()),
            proposals: Vector::new(b"p".to_vec()),
            last_voted: UnorderedMap::new(b"e".to_vec()),
            protocol_address
        };
        for account_id in council.clone() {
            dao.council.insert(&account_id);
        }
        dao
    }

    #[payable]
    pub fn add_proposal(&mut self, proposal: ProposalInput) -> U64 {
        // TODO: add also extra storage cost for the proposal itself.
        assert!(
            proposal.description.len() < MAX_DESCRIPTION_LENGTH,
            "Description length is too long"
        );
        assert!(
            self.council.contains(&env::predecessor_account_id()),
            "Only council can create proposals"
        );
        assert!(env::attached_deposit() >= self.bond, "Not enough deposit");

        let p = Proposal {
            status: ProposalStatus::Vote,
            proposer: env::predecessor_account_id(),
            description: proposal.description,
            kind: proposal.kind,
            last_vote: 0,
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

    fn update_vote_status(&self, proposal: &mut Proposal) {
        proposal.status = match proposal.kind {
            ProposalKind::ResoluteMarket{ ref market_id, ref payout_numerator } => {
                proposal.vote_status(&RESOLUTE_POLICY, self.council.len())
            }
            _ => {
                proposal.vote_status(&self.policy, self.council.len())
            }
        }
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
            "Proposal not active voting"
        );
        assert!(proposal.vote_period_end > env::block_timestamp(), "voting period ended");
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
        self.update_vote_status(&mut proposal);
        proposal.last_vote = env::block_timestamp();
        self.proposals.replace(id.into(), &proposal);
    }

    fn proposal_success(&mut self, id: u64, proposal: &mut Proposal, bond: u128){
        assert!(proposal.status == ProposalStatus::Success, "Wrong status on callback");
        proposal.status = ProposalStatus::Finalized;
        self.proposals.replace(id, &proposal);

        if bond > 0 {
            Promise::new(proposal.proposer.clone()).transfer(bond);
        }
    }

    pub fn ft_resolve_protocol_call(
        &mut self,
        id: U64
    ) {
        utils::assert_self();
        let mut proposal = self.proposals.get(id.into()).expect("No proposal with such id");
        match env::promise_result(0) {
            PromiseResult::NotReady => unreachable!(),
            PromiseResult::Successful(value) => {
                self.proposal_success(id.into(), &mut proposal, self.bond)
            }
            PromiseResult::Failed => {},
        };
    }

    pub fn finalize_external(&mut self, id: U64) -> Promise {
        let mut proposal = self.proposals.get(id.into()).expect("No proposal with such id");
        assert!(
            !proposal.status.is_finished(),
            "Proposal already finalized"
        );
        match proposal.kind {
            ProposalKind::PauseProtocol{ } => {
                // no grace period
            }
            ProposalKind::UnpauseProtocol{ } => {
                // no grace period
            }
            _ => {
                assert!(env::block_timestamp() > proposal.last_vote + self.grace_period, "Grace period active");
            }
        }
        self.update_vote_status(&mut proposal);
        self.proposals.replace(id.into(), &proposal);
        let prom: Promise = match proposal.status {
            ProposalStatus::Success => {
                match proposal.kind {
                    ProposalKind::ResoluteMarket{ ref market_id, ref payout_numerator } => {
                        // base gas + gas for each enumerator
                        let resolute_gas = match payout_numerator {
                            Some(payout_vec) => payout_vec.len() as u64 * RESOLUTION_GAS,
                            None => RESOLUTION_GAS
                        };
                        flux_protocol::resolute_market(
                            *market_id,
                            payout_numerator.clone(),
                            &self.protocol_address,
                            0,
                            resolute_gas,
                        )
                    },
                    ProposalKind::SetTokenWhitelist{ ref whitelist } => {
                        flux_protocol::set_token_whitelist(
                            whitelist.clone(),
                            &self.protocol_address,
                            0,
                            RESOLUTION_GAS,
                        )
                    },
                    ProposalKind::AddTokenWhitelist{ ref to_add } => {
                        flux_protocol::add_to_token_whitelist(
                            to_add.clone(),
                            &self.protocol_address,
                            0,
                            RESOLUTION_GAS,
                        )
                    },
                    ProposalKind::SetGov{ ref new_gov } => {
                        flux_protocol::set_gov(
                            new_gov.clone(),
                            &self.protocol_address,
                            0,
                            RESOLUTION_GAS,
                        )
                    },
                    ProposalKind::PauseProtocol{ } => {
                        flux_protocol::pause(
                            &self.protocol_address,
                            0,
                            RESOLUTION_GAS,
                        )
                    },
                    ProposalKind::UnpauseProtocol{ } => {
                        flux_protocol::unpause(
                            &self.protocol_address,
                            0,
                            RESOLUTION_GAS,
                        )
                    },
                    _ => {
                        env::panic(b"not an external proposal")
                    }
                }
            }
            ProposalStatus::Reject => {
                proposal.status = ProposalStatus::Rejected;
                self.proposals.replace(id.into(), &proposal);
                Promise::new(proposal.proposer.clone()).transfer(self.bond)
            }
            _ => {
                env::panic(b"voting period has not expired and no majority vote yet")
            }
        };

        if proposal.status == ProposalStatus::Success {
            prom.then(ext_self::ft_resolve_protocol_call(
                id,
                &env::current_account_id(),
                0,
                RESOLUTION_GAS,
            ))
        } else {
            prom
        }
    }

    pub fn finalize(&mut self, id: U64) {
        let mut proposal = self.proposals.get(id.into()).expect("No proposal with such id");
        assert!(
            !proposal.status.is_finished(),
            "Proposal already finalized"
        );
        match proposal.kind {
            ProposalKind::PauseProtocol{ } => {
                // no grace period
            }
            ProposalKind::UnpauseProtocol{ } => {
                // no grace period
            }
            _ => {
                assert!(env::block_timestamp() > proposal.last_vote + self.grace_period, "Grace period active");
            }
        }
        self.update_vote_status(&mut proposal);
        let actual_bond = self.bond;
        match proposal.status {
            ProposalStatus::Success => {
                // env::log(b"Vote succeeded");
                match proposal.kind {
                    ProposalKind::NewCouncil { ref target } => {
                        self.council.insert(&target.clone());
                    }
                    ProposalKind::RemoveCouncil { ref target } => {
                        self.kick_user(&target.clone());
                    }
                    ProposalKind::Payout { ref target, amount } => {
                        Promise::new(target.clone()).transfer(amount.0);
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
                    },
                    ProposalKind::ChangeProtocolAddress{ ref address } => {
                        self.protocol_address = address.to_string();
                    },
                    _ => {
                        env::panic(b"not an internal proposal")
                    }
                }
            }
            ProposalStatus::Reject => {
                proposal.status = ProposalStatus::Rejected;
                Promise::new(proposal.proposer.clone()).transfer(self.bond);
            }
            _ => {
                env::panic(b"voting period has not expired and no majority vote yet")
            }
        };

        self.proposals.replace(id.into(), &proposal);
        if proposal.status == ProposalStatus::Success{
            self.proposal_success(id.into(), &mut proposal, actual_bond);
        }
    }

    pub fn exit_dao(&mut self) {
        self.kick_user(&env::predecessor_account_id());
    }

    fn kick_user(&mut self, account_id: &AccountId) {
        let proposalid = self.last_voted.get(account_id);
        if !proposalid.is_none() {
            let proposal = self.proposals.get(proposalid.unwrap()).expect("ERR_PROPOSAL_NOT_FOUND");

            match proposal.kind {
                ProposalKind::RemoveCouncil { target } => {
                    if &target != account_id {
                        assert!(proposal.status != ProposalStatus::Vote, "ERR_VOTING_ACTIVE");
                    }
                },
                _ => {
                    assert!(proposal.status != ProposalStatus::Vote, "ERR_VOTING_ACTIVE");
                }
            }
        }
        assert!(self.council.remove(account_id), "ERR_NOT_IN_COUNCIL");
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
    fn dave() -> AccountId {
        "dave.near".to_string()
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
            U64(10),
            U64(10),
            protocol_address()
        );
        dao
    }

    fn poll_finalize(contract : &mut FluxDAO, id: U64) {
        let mut context = get_context(alice());
        context.block_timestamp = 50000;
        testing_env!(context);
        contract.finalize(id);
    }

    fn add_bob(contract : &mut FluxDAO) {
        let proposal = ProposalInput {
            description:  String::from("add bob"),
            kind: ProposalKind::NewCouncil { target: bob() },
        };
        let index:U64 = contract.add_proposal(proposal);
        contract.vote(index, Vote::Yes);

        let mut context = get_context(alice());
        context.block_timestamp = 10000;
        testing_env!(context);
        contract.finalize(index);
    }

    fn add_carol(contract : &mut FluxDAO) {
        let mut context = get_context(alice());
        // todo remove
        context.block_timestamp = 100;
        context.attached_deposit = to_yocto(1000);
        testing_env!(context);

        let proposal = ProposalInput {
            description:  String::from("add carol"),
            kind: ProposalKind::NewCouncil{ target: carol() },
        };
        let index:U64 = contract.add_proposal(proposal);
        contract.vote(index, Vote::Yes);

        // todo verify
        let mut context = get_context(bob());
        testing_env!(context);
        contract.vote(index, Vote::Yes);

        let mut context = get_context(alice());
        context.block_timestamp = 20000;
        testing_env!(context);
        contract.finalize(index);
    }

    fn add_dave(contract : &mut FluxDAO) {
        let mut context = get_context(alice());
        // todo remove
        context.block_timestamp = 100;
        context.attached_deposit = to_yocto(1000);
        testing_env!(context);

        let proposal = ProposalInput {
            description:  String::from("add dave"),
            kind: ProposalKind::NewCouncil{ target: dave() },
        };
        let index:U64 = contract.add_proposal(proposal);
        contract.vote(index, Vote::Yes);

        // todo verify
        let mut context = get_context(carol());
        testing_env!(context);
        contract.vote(index, Vote::Yes);

        // let mut context = get_context(bob());
        // testing_env!(context);
        // contract.vote(index, Vote::Yes);

        let mut context = get_context(alice());
        context.block_timestamp = 20000;
        testing_env!(context);
        contract.finalize(index);
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
        assert_eq!(contract.policy.max_amount, U128(0));
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
            U128(to_yocto(2)),
            U64(0),
            U64(0),
            protocol_address()
        );
        let proposal = ProposalInput {
            description: String::from("carol is cool"),
            kind: ProposalKind::NewCouncil{target: carol() },
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
            description: String::from("a").repeat(281),
            kind: ProposalKind::NewCouncil { target: carol() },
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
            description: description.clone(),
            kind: ProposalKind::NewCouncil{ target: carol() },
        };

        // Carol (not in council) creates a proposal to include her in the counsil
        let index:U64 = contract.add_proposal(proposal);
        // TODO, verify contract balance in NEAR
        assert_eq!(index, U64(0));
        assert_eq!(contract.get_num_proposals(), U64(1));
        let mut proposal = contract.get_proposal(U64(0));
        assert_eq!(proposal.status, ProposalStatus::Vote);
        assert_eq!(proposal.proposer, alice());
        //assert_eq!(proposal.kind.target, carol());
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

        poll_finalize(&mut contract, U64(0));
        proposal = contract.get_proposal(U64(0));

        assert_eq!(proposal.vote_yes, 1);
        assert_eq!(proposal.vote_no, 0);
        assert_eq!(proposal.status, ProposalStatus::Finalized);
        assert_eq!(proposal.proposer, alice());
       // assert_eq!(proposal.kind.target, carol());
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
            description: description.clone(),
            kind: ProposalKind::NewCouncil{target: bob() },
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
            description: description.clone(),
            kind: ProposalKind::RemoveCouncil{target: bob()},
        };
        let index:U64 = contract.add_proposal(proposal);

        assert_eq!(contract.council.len(), 3);

        let mut context = get_context(alice());
        testing_env!(context);
        contract.vote(index, Vote::Yes);

        let mut context = get_context(carol());

        testing_env!(context);
        contract.vote(index, Vote::Yes);
        context = get_context(alice());
        // TODO, is sending near expected in this case
        // this amount pays out the exit bond
        context.attached_deposit = to_yocto(5000);
        context.block_timestamp = 50000;
        testing_env!(context);
        contract.finalize(index);

        assert_eq!(contract.council.len(), 2);
    }

    #[test]
    fn test_remove_council_proposal_voteself() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);
        let mut contract = init();
        assert_eq!(contract.council.len(), 1);
        add_bob(&mut contract);
        assert_eq!(contract.council.len(), 2);
        add_carol(&mut contract);
        let description = String::from("bob sucks");
        let proposal = ProposalInput {
            description: description.clone(),
            kind: ProposalKind::RemoveCouncil{target:bob()},
        };
        let index:U64 = contract.add_proposal(proposal);
        assert_eq!(contract.council.len(), 3);

        let mut context = get_context(alice());
        testing_env!(context);
        contract.vote(index, Vote::Yes);

        let mut context = get_context(bob());
        testing_env!(context);
        contract.vote(index, Vote::Yes);

        context = get_context(alice());
        // todo check deposit
        context.attached_deposit = to_yocto(5000);
        context.block_timestamp = 50000;
        testing_env!(context);
        contract.finalize(index);

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
            description: description.clone(),
            kind: ProposalKind::Payout{ target: bob(), amount: U128(to_yocto(1)) },
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
            description: description.clone(),
            kind: ProposalKind::ChangeVotePeriod{ vote_period: U64(1) },
        };
        contract.add_proposal(proposal);
        assert_eq!(contract.get_vote_period(), U64(10));
        contract.vote(U64(0), Vote::Yes);

        poll_finalize(&mut contract, U64(0));
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
            description: description.clone(),
            kind: ProposalKind::ChangeBond{ bond: U128(1) },
        };
        contract.add_proposal(proposal);
        assert_eq!(contract.get_bond(), U128(0));
        contract.vote(U64(0), Vote::Yes);

        poll_finalize(&mut contract, U64(0));
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
        let policy = PolicyItem {
            max_amount: 100.into(),
            votes: NumOrRatio::Ratio(1, 2),
        };
        let proposal = ProposalInput {
            description: description.clone(),
            kind: ProposalKind::ChangePolicy{ policy },
        };
        contract.add_proposal(proposal);
        assert_eq!(contract.policy.max_amount, U128(0));
        contract.vote(U64(0), Vote::Yes);

        poll_finalize(&mut contract, U64(0));
        assert_eq!(contract.policy.max_amount, U128(100));
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
            description: description.clone(),
            kind: ProposalKind::ChangePurpose{ purpose: description.clone() },
        };
        contract.add_proposal(proposal);
        assert_eq!(contract.purpose, purpose);
        contract.vote(U64(0), Vote::Yes);

        poll_finalize(&mut contract, U64(0));
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
            description: description.clone(),
            kind: ProposalKind::ChangeBond{ bond: U128(1) },
        };
        let index:U64 = contract.add_proposal(proposal);

        assert_eq!(contract.get_bond(), U128(0));
        contract.vote(index, Vote::No);

        poll_finalize(&mut contract, index);
        let p:Proposal = contract.get_proposal(index);
        assert_eq!(p.status, ProposalStatus::Rejected);
        assert_eq!(contract.get_bond(), U128(0));
        // TODO, check balance
    }

    #[test]
    #[should_panic(expected = "Only council can vote")]
    fn test_no_council_vote() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);

        let mut contract = init();
        let proposal = ProposalInput {
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
    #[should_panic(expected = "Proposal not active voting")]
    fn test_proposal_already_finalized() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);

        let mut contract = init();
        let proposal = ProposalInput {
            description: String::from("x"),
            kind: ProposalKind::ChangePurpose{ purpose:String::from("y") },
        };
        contract.add_proposal(proposal);
        contract.vote(U64(0), Vote::Yes);

        poll_finalize(&mut contract, U64(0));
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

        let protocol_new : AccountId = "protocol2".to_string();
        let mut contract = init();
        assert_eq!(contract.protocol_address, protocol_address());
        let proposal = ProposalInput {
            description: String::from("change protocol address"),
            kind: ProposalKind::ChangeProtocolAddress{ address: protocol_new.clone() }
        };
        contract.add_proposal(proposal);
        contract.vote(U64(0), Vote::Yes);
        poll_finalize(&mut contract, U64(0));
        assert_eq!(contract.protocol_address, protocol_new.clone());
    }

    #[test]
    #[should_panic(expected = "Grace period active")]
    fn test_grace_period_active() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);

        let protocol_new : AccountId = "protocol".to_string();
        let mut contract = init();
        assert_eq!(contract.protocol_address, protocol_address());
        let proposal = ProposalInput {
            description: String::from("change protocol address"),
            kind: ProposalKind::ChangeProtocolAddress{ address: protocol_new.clone() }
        };
        contract.add_proposal(proposal);
        contract.vote(U64(0), Vote::Yes);

        let mut context = get_context(alice());
        context.block_timestamp = 5;
        testing_env!(context);
        contract.finalize(U64(0));
    }

    #[test]
    fn test_pause_protocol() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);
        let mut contract = init();
        assert_eq!(contract.protocol_address, protocol_address());
        let proposal = ProposalInput {
            description: String::from("pause protocol"),
            kind: ProposalKind::PauseProtocol{ }
        };
        let id = contract.add_proposal(proposal);
        contract.vote(U64(0), Vote::Yes);
        // dont chagnge block number after voting (e.g. no grace period)
        contract.finalize_external(id);
    }

    #[test]
    fn test_unpause_protocol() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);
        let mut contract = init();
        assert_eq!(contract.protocol_address, protocol_address());
        let proposal = ProposalInput {
            description: String::from("pause protocol"),
            kind: ProposalKind::UnpauseProtocol{ }
        };
        let id = contract.add_proposal(proposal);
        contract.vote(U64(0), Vote::Yes);
        // dont chagnge block number after voting (e.g. no grace period)
        contract.finalize_external(id);
    }

    #[test]
    fn test_resolute_policy() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);
        let mut contract = init();
        add_bob(&mut contract);
        add_carol(&mut contract);
        add_dave(&mut contract);
        let proposal = ProposalInput {
            description: String::from("pause protocol"),
            kind: ProposalKind::ResoluteMarket{
                market_id: U64(0),
                payout_numerator: None
            }
        };
        // vote #1
        let id = contract.add_proposal(proposal);
        contract.vote(id, Vote::Yes);
        // vote #2
        let mut context = get_context(bob());
        testing_env!(context);
        contract.vote(id, Vote::Yes);
        // vote #3
        let mut context = get_context(carol());
        testing_env!(context);
        contract.vote(id, Vote::Yes);
        // verify vote
        let p:Proposal = contract.get_proposal(id);
        assert_eq!(p.status, ProposalStatus::Vote);
        // vote #4
        let mut context = get_context(dave());
        testing_env!(context);
        contract.vote(id, Vote::Yes);
        // finalize
        let mut context = get_context(alice());
        context.block_timestamp = 50000;
        testing_env!(context);
        contract.finalize_external(id);
        // verify state
        let p:Proposal = contract.get_proposal(id);
        assert_eq!(p.status, ProposalStatus::Success);
    }

    #[test]
    fn test_resolute_policy_fail() {
        let mut context = get_context(alice());
        context.attached_deposit = to_yocto(5000);
        testing_env!(context);
        let mut contract = init();
        add_bob(&mut contract);
        add_carol(&mut contract);
        add_dave(&mut contract);
        let proposal = ProposalInput {
            description: String::from("pause protocol"),
            kind: ProposalKind::ResoluteMarket{
                market_id: U64(0),
                payout_numerator: None
            }
        };
        // vote #1
        let id = contract.add_proposal(proposal);
        contract.vote(id, Vote::Yes);
        // vote #2
        let mut context = get_context(bob());
        testing_env!(context);
        contract.vote(id, Vote::Yes);
        // vote #3
        let mut context = get_context(carol());
        testing_env!(context);
        contract.vote(id, Vote::Yes);
        // verify vote
        let p:Proposal = contract.get_proposal(id);
        assert_eq!(p.status, ProposalStatus::Vote);
        // finalize
        let mut context = get_context(alice());
        context.block_timestamp = 50000;
        testing_env!(context);
        contract.finalize_external(id);
        // verify state
        let p:Proposal = contract.get_proposal(id);
        assert_eq!(p.status, ProposalStatus::Rejected);
    }
}