use crate::policy_item::{ PolicyItem };
use near_sdk::{ Balance };

pub fn vote_requirement(policy: &[PolicyItem], num_council: u64, amount: Option<Balance>) -> u64 {
    if let Some(amount) = amount {
        // TODO: replace with binary search.
        for item in policy {
            if item.max_amount.0 > amount {
                return item.num_votes(num_council);
            }
        }
    }
    policy[policy.len() - 1].num_votes(num_council)
}

pub fn to_yocto(value: u128) -> u128 {
    value * 10_u128.pow(24)
}
