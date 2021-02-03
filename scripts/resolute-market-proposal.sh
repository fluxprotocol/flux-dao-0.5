. ./config.sh

JSON='{"proposal": {"market_id": "'${1}'", "payout_numerator": '${2}'}}'

near call $TOKEN_CONTRACT add_proposal $JSON --accountId $3