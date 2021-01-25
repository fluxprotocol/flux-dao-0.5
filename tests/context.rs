use near_sdk::{
    AccountId,
    serde_json::json,
    json_types::{
        U128,
        U64
    }
};
use near_sdk_sim::{
    call,
    deploy,
    init_simulator,
    to_yocto,
    view,
    ContractAccount,
    UserAccount,
    STORAGE_AMOUNT,
    DEFAULT_GAS,
    ExecutionResult,
    account::AccessKey
};

extern crate flux_dao;
use flux_dao::FluxDAOContract;
use flux_dao::{ProposalInput, ProposalKind, Vote};

const REGISTRY_STORAGE: u128 = 8_300_000_000_000_000_000_000;
const MINIMAL_NEAR_FOR_COUNCIL: &str = "1000";

near_sdk_sim::lazy_static! {
    static ref DAO_WASM_BYTES: &'static [u8] = include_bytes!("../res/flux_dao.wasm").as_ref();
    static ref FLUX_WASM_BYTES: &'static [u8] = include_bytes!("../res/flux_protocol.wasm").as_ref();
}

// todo, constructor also need to be u64->U64, u128->U128?
fn init(
    initial_balance: u128,
    purpose: String,
    council: Vec<AccountId>,
    bond: u128,
    vote_period: u64,
    grace_period: u64,
) -> (UserAccount, ContractAccount<FluxDAOContract>, UserAccount, UserAccount, UserAccount) {
    let master_account = init_simulator(None);
    let dao_contract = deploy!(
        // Contract Proxy
        contract: FluxDAOContract,
        // Contract account id
        contract_id: "dao",
        // Bytes of contract
        bytes: &DAO_WASM_BYTES,
        // User deploying the contract,
        signer_account: master_account,
        // init method
        init_method: new(
            purpose,
            U128(bond),
            U64(vote_period),
            U64(grace_period),
            protocol_address()
        )
    );

    let alice = master_account.create_user(alice(), to_yocto("1000"));
    let bob = master_account.create_user(bob(), to_yocto("100"));
    let carol = master_account.create_user(carol(), to_yocto("100"));

    let token_contract = master_account.create_user(protocol_address(), to_yocto("100"));
    let tx = token_contract.create_transaction(token_contract.account_id());
    // uses default values for deposit and gas
    let res = tx
        .transfer(to_yocto("1"))
        .deploy_contract((&FLUX_WASM_BYTES).to_vec())
        .submit();

    // transfer some NEAR to alice
    let tx2 = master_account.create_transaction(alice.account_id());
    let res2 = tx2
        .transfer(to_yocto(MINIMAL_NEAR_FOR_COUNCIL))
        .submit();

    init_protocol(&token_contract);

    let res = call!(
        master_account,
        dao_contract.init(council),
        deposit = to_yocto("5000")
    );
    assert!(res.is_ok());

    (master_account, dao_contract, alice, bob, carol)
}

fn init_protocol(
    protocol_contract: &UserAccount,
) {
    let tx = protocol_contract.create_transaction(protocol_contract.account_id());
    let args = json!({}).to_string().as_bytes().to_vec();
    let res = tx.function_call("init".into(), args, DEFAULT_GAS, 0).submit();
    if !res.is_ok() {
        panic!("token initiation failed: {:?}", res);
    }
}

fn alice() -> String {
    "alice".to_string()
}

fn bob() -> String {
    "bob".to_string()
}

fn carol() -> String {
    "carol".to_string()
}

fn target() -> String {
    "target".to_string()
}

fn description() -> String {
    "description".to_string()
}

fn protocol_address() -> String {
    "protocol".to_string()
}

#[test]
fn test_init() {
    let (master_account, dao, c1, c2, c3) = init(
        to_yocto("100000000"),
        "testing".to_string(),
        vec![alice(), bob()],
        to_yocto("1"),
        12938120938,
        12837129837
    );
}

#[test]
fn test_new_proposal() {
    let (master_account, dao, c1, c2, c3) = init(
        to_yocto("100000000"),
        "testing".to_string(),
        vec![alice(), bob()],
        to_yocto("1"),
        12938120938,
        12837129837
    );

    let proposal = ProposalInput {
        target: c3.account_id(),
        description: description(),
        kind: ProposalKind::NewCouncil,
    };

    let res = call!(
        c1,
        dao.add_proposal(proposal),
        deposit = to_yocto(MINIMAL_NEAR_FOR_COUNCIL)
    );

    println!("res: {:?}", res);

    assert!(res.is_ok());
}

#[test]
fn test_cross_contract_resolution() {
    let (master_account, dao, c1, c2, c3) = init(
        to_yocto("100000000"),
        "testing".to_string(),
        vec![alice(), bob()],
        to_yocto("1"),
        12938120938,
        12837129837
    );

    let proposal = ProposalInput {
        target: c3.account_id(),
        description: description(),
        kind: ProposalKind::ResoluteMarket{
            market_id: U64(0),
            payout_numerator: None
        },
    };

    let proposal_id: U64 = call!(
        c1,
        dao.add_proposal(proposal),
        deposit = to_yocto(MINIMAL_NEAR_FOR_COUNCIL)
    ).unwrap_json();

    let vote_res_c1 = call!(
        c1,
        dao.vote(proposal_id, Vote::Yes),
        deposit = 0
    );

    assert!(vote_res_c1.is_ok());

    let vote_res_c2 = call!(
        c2,
        dao.vote(proposal_id, Vote::Yes),
        deposit = 0
    );
    println!("vote {:?}", vote_res_c2);
    assert!(vote_res_c2.is_ok());
}

#[test]
fn test_cross_contract_set_whitelist() {
    let (master_account, dao, c1, c2, c3) = init(
        to_yocto("100000000"),
        "testing".to_string(),
        vec![alice(), bob()],
        to_yocto("1"),
        12938120938,
        12837129837
    );

    let proposal = ProposalInput {
        target: c3.account_id(),
        description: description(),
        kind: ProposalKind::SetTokenWhitelist{
            whitelist: vec![alice(), bob()]
        },
    };

    let proposal_id: U64 = call!(
        c1,
        dao.add_proposal(proposal),
        deposit = to_yocto(MINIMAL_NEAR_FOR_COUNCIL)
    ).unwrap_json();

    let vote_res_c1 = call!(
        c1,
        dao.vote(proposal_id, Vote::Yes),
        deposit = 0
    );

    assert!(vote_res_c1.is_ok());

    let vote_res_c2 = call!(
        c2,
        dao.vote(proposal_id, Vote::Yes),
        deposit = 0
    );
    println!("vote {:?}", vote_res_c2);
    assert!(vote_res_c2.is_ok());
}

#[test]
fn test_cross_contract_add_to_whitelist() {
    let (master_account, dao, c1, c2, c3) = init(
        to_yocto("100000000"),
        "testing".to_string(),
        vec![alice(), bob()],
        to_yocto("1"),
        12938120938,
        12837129837
    );

    let proposal = ProposalInput {
        target: c3.account_id(),
        description: description(),
        kind: ProposalKind::AddTokenWhitelist{
            to_add: bob()
        },
    };

    let proposal_id: U64 = call!(
        c1,
        dao.add_proposal(proposal),
        deposit = to_yocto(MINIMAL_NEAR_FOR_COUNCIL)
    ).unwrap_json();

    let vote_res_c1 = call!(
        c1,
        dao.vote(proposal_id, Vote::Yes),
        deposit = 0
    );

    assert!(vote_res_c1.is_ok());

    let vote_res_c2 = call!(
        c2,
        dao.vote(proposal_id, Vote::Yes),
        deposit = 0
    );
    println!("vote {:?}", vote_res_c2);
    assert!(vote_res_c2.is_ok());
}