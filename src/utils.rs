use crate::policy_item::{ PolicyItem };
use near_sdk::{ env, Balance };

pub (crate) fn to_yocto(value: u128) -> u128 {
    value * 10_u128.pow(24)
}


pub(crate) fn assert_self() {
    assert_eq!(
        env::predecessor_account_id(),
        env::current_account_id(),
        "Method is private"
    );
}