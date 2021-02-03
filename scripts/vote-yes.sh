. ./config.sh

JSON='{"id": "'${1}'", "vote": 0}'

near call $TOKEN_CONTRACT vote $JSON --accountId $2