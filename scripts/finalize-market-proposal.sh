. ./config.sh

JSON='{"id": "'${1}'"}'

near call $TOKEN_CONTRACT finalize $JSON --accountId $2