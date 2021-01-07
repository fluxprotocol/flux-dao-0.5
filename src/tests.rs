mod context;

use crate::FluxDAO;
use crate::types::{ Vote };
use crate::proposal_status::{ ProposalStatus };
use crate::proposal::{ ProposalInput, ProposalKind };
use crate::policy_item::{ PolicyItem };
use crate::types::{ NumOrRatio };

#[cfg(test)]
mod tests {
    use context::{accounts, VMContextBuilder};
    use near_sdk::{MockedBlockchain, testing_env};

    use super::*;

    fn vote(dao: &mut FluxDAO, proposal_id: u64, votes: Vec<(usize, Vote)>) {
        for (id, vote) in votes {
            testing_env!(VMContextBuilder::new()
                .predecessor_account_id(accounts(id))
                .finish());
            dao.vote(proposal_id, vote);
        }
    }

    #[test]
    fn test_basics() {
        testing_env!(VMContextBuilder::new().finish());
        let mut dao = FluxDAO::new(
            "test".to_string(),
            vec![accounts(0), accounts(1)],
            10.into(),
            1_000.into(),
            10.into(),
        );

        assert_eq!(dao.get_bond(), 10.into());
        assert_eq!(dao.get_vote_period(), 1_000.into());
        assert_eq!(dao.get_purpose(), "test");

        testing_env!(VMContextBuilder::new()
            .predecessor_account_id(accounts(2))
            .attached_deposit(10)
            .finish());
        let id = dao.add_proposal(ProposalInput {
            target: accounts(2),
            description: "add new member".to_string(),
            kind: ProposalKind::NewCouncil,
        });
        assert_eq!(dao.get_num_proposals(), 1);
        assert_eq!(dao.get_proposals(0, 1).len(), 1);
        vote(&mut dao, id, vec![(0, Vote::Yes)]);
        assert_eq!(dao.get_proposal(id).vote_yes, 1);
        assert_eq!(dao.get_proposal(id).status, ProposalStatus::Vote);
        assert_eq!(dao.get_council(), vec![accounts(0), accounts(1)]);
        vote(&mut dao, id, vec![(1, Vote::Yes)]);
        assert_eq!(
            dao.get_council(),
            vec![accounts(0), accounts(1), accounts(2)]
        );

        // Pay out money for proposal. 2 votes yes vs 1 vote no.
        testing_env!(VMContextBuilder::new()
            .predecessor_account_id(accounts(2))
            .attached_deposit(10)
            .finish());
        let id = dao.add_proposal(ProposalInput {
            target: accounts(2),
            description: "give me money".to_string(),
            kind: ProposalKind::Payout { amount: 10.into() },
        });
        vote(
            &mut dao,
            id,
            vec![(0, Vote::No), (1, Vote::Yes), (2, Vote::Yes)],
        );
        assert_eq!(dao.get_proposal(id).vote_yes, 2);
        assert_eq!(dao.get_proposal(id).vote_no, 1);
        assert_eq!(dao.get_proposal(id).status, ProposalStatus::Success);

        // No vote for proposal.
        testing_env!(VMContextBuilder::new()
            .predecessor_account_id(accounts(2))
            .attached_deposit(10)
            .finish());
        let id = dao.add_proposal(ProposalInput {
            target: accounts(2),
            description: "give me more money".to_string(),
            kind: ProposalKind::Payout { amount: 10.into() },
        });
        testing_env!(VMContextBuilder::new()
            .predecessor_account_id(accounts(3))
            .block_timestamp(1_001)
            .finish());
        dao.finalize(id);
        assert_eq!(dao.get_proposal(id).status, ProposalStatus::Fail);

        // Change policy.
        testing_env!(VMContextBuilder::new()
            .predecessor_account_id(accounts(2))
            .attached_deposit(10)
            .finish());
        let id = dao.add_proposal(ProposalInput {
            target: accounts(2),
            description: "policy".to_string(),
            kind: ProposalKind::ChangePolicy{ policy: vec![
                PolicyItem {
                    max_amount: 100.into(),
                    votes: NumOrRatio::Number(1),
                },
                PolicyItem {
                    max_amount: 1_000_000.into(),
                    votes: NumOrRatio::Ratio(1, 1),
                },
            ]},
        });
        vote(&mut dao, id, vec![(0, Vote::Yes), (1, Vote::Yes)]);

        // Try new policy with small amount.
        testing_env!(VMContextBuilder::new()
            .predecessor_account_id(accounts(2))
            .attached_deposit(10)
            .finish());
        let id = dao.add_proposal(ProposalInput {
            target: accounts(2),
            description: "give me more money".to_string(),
            kind: ProposalKind::Payout { amount: 10.into() },
        });
        vote(&mut dao, id, vec![(0, Vote::Yes)]);
        assert_eq!(dao.get_proposal(id).status, ProposalStatus::Delay);
        testing_env!(VMContextBuilder::new()
            .predecessor_account_id(accounts(3))
            .block_timestamp(11)
            .finish());
        dao.finalize(id);
        assert_eq!(dao.get_proposal(id).status, ProposalStatus::Success);

        // New policy for bigger amounts requires 100% votes.
        testing_env!(VMContextBuilder::new()
            .predecessor_account_id(accounts(2))
            .attached_deposit(10)
            .finish());
        let id = dao.add_proposal(ProposalInput {
            target: accounts(2),
            description: "give me more money".to_string(),
            kind: ProposalKind::Payout {
                amount: 10_000.into(),
            },
        });
        vote(&mut dao, id, vec![(0, Vote::Yes)]);
        assert_eq!(dao.get_proposal(id).status, ProposalStatus::Vote);
        vote(&mut dao, id, vec![(1, Vote::Yes)]);
        assert_eq!(dao.get_proposal(id).status, ProposalStatus::Vote);
        vote(&mut dao, id, vec![(2, Vote::Yes)]);
        assert_eq!(dao.get_proposal(id).status, ProposalStatus::Success);
    }

    #[test]
    fn test_single_council() {
        testing_env!(VMContextBuilder::new().finish());
        let mut dao = FluxDAO::new(
            "".to_string(),
            vec![accounts(0)],
            10.into(),
            1_000.into(),
            10.into(),
        );

        testing_env!(VMContextBuilder::new()
            .predecessor_account_id(accounts(2))
            .attached_deposit(10)
            .finish());
        let id = dao.add_proposal(ProposalInput {
            target: accounts(1),
            description: "add new member".to_string(),
            kind: ProposalKind::NewCouncil,
        });
        vote(&mut dao, id, vec![(0, Vote::Yes)]);
        assert_eq!(dao.get_proposal(id).status, ProposalStatus::Success);
        assert_eq!(dao.get_council(), vec![accounts(0), accounts(1)]);
    }

    #[test]
    #[should_panic]
    fn test_double_vote() {
        testing_env!(VMContextBuilder::new().finish());
        let mut dao = FluxDAO::new(
            "".to_string(),
            vec![accounts(0), accounts(1)],
            10.into(),
            1000.into(),
            10.into(),
        );
        testing_env!(VMContextBuilder::new()
            .predecessor_account_id(accounts(2))
            .attached_deposit(10)
            .finish());
        let id = dao.add_proposal(ProposalInput {
            target: accounts(2),
            description: "add new member".to_string(),
            kind: ProposalKind::NewCouncil,
        });
        assert_eq!(dao.get_proposals(0, 1).len(), 1);
        testing_env!(VMContextBuilder::new()
            .predecessor_account_id(accounts(0))
            .finish());
        dao.vote(id, Vote::Yes);
        dao.vote(id, Vote::Yes);
    }

    #[test]
    fn test_two_council() {
        testing_env!(VMContextBuilder::new().finish());
        let mut dao = FluxDAO::new(
            "".to_string(),
            vec![accounts(0), accounts(1)],
            10.into(),
            1_000.into(),
            10.into(),
        );

        testing_env!(VMContextBuilder::new()
            .predecessor_account_id(accounts(2))
            .attached_deposit(10)
            .finish());
        let id = dao.add_proposal(ProposalInput {
            target: accounts(1),
            description: "add new member".to_string(),
            kind: ProposalKind::Payout { amount: 100.into() },
        });
        vote(&mut dao, id, vec![(0, Vote::Yes), (1, Vote::No)]);
        assert_eq!(dao.get_proposal(id).status, ProposalStatus::Fail);
    }

    #[test]
    #[should_panic]
    fn test_run_out_of_money() {
        testing_env!(VMContextBuilder::new().finish());
        let mut dao = FluxDAO::new(
            "".to_string(),
            vec![accounts(0)],
            10.into(),
            1000.into(),
            10.into(),
        );
        testing_env!(VMContextBuilder::new()
            .predecessor_account_id(accounts(2))
            .attached_deposit(10)
            .finish());
        let id = dao.add_proposal(ProposalInput {
            target: accounts(2),
            description: "add new member".to_string(),
            kind: ProposalKind::Payout { amount: 1000.into() },
        });
        assert_eq!(dao.get_proposals(0, 1).len(), 1);
        testing_env!(VMContextBuilder::new()
            .predecessor_account_id(accounts(0))
            .account_balance(10)
            .finish());
        dao.vote(id, Vote::Yes);
    }

    #[test]
    #[should_panic]
    fn test_incorrect_policy() {
        testing_env!(VMContextBuilder::new().finish());
        let mut dao = FluxDAO::new(
            "".to_string(),
            vec![accounts(0), accounts(1)],
            10.into(),
            1000.into(),
            10.into(),
        );
        testing_env!(VMContextBuilder::new()
            .predecessor_account_id(accounts(2))
            .attached_deposit(10)
            .finish());
        dao.add_proposal(ProposalInput {
            target: accounts(2),
            description: "policy".to_string(),
            kind: ProposalKind::ChangePolicy{ policy: vec![
                PolicyItem {
                    max_amount: 100.into(),
                    votes: NumOrRatio::Number(5),
                },
                PolicyItem {
                    max_amount: 5.into(),
                    votes: NumOrRatio::Number(3),
                },
            ]},
        });
    }
}
