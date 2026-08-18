#!/usr/bin/env bash
set -euo pipefail

DOMAIN="${OSIR_DNS_DOMAIN:-osirclaw.com}"
RR="${OSIR_DNS_RR:-app}"
VALUE="${OSIR_DNS_VALUE:-154.40.47.227}"
TTL="${OSIR_DNS_TTL:-600}"
command -v aliyun >/dev/null || { echo "aliyun CLI is required" >&2; exit 2; }
command -v jq >/dev/null || { echo "jq is required" >&2; exit 2; }

aliyun_call() {
  if [[ -n "${ALIYUN_PROFILE:-}" ]]; then
    aliyun --profile "${ALIYUN_PROFILE}" "$@"
  else
    aliyun "$@"
  fi
}

records="$(aliyun_call alidns DescribeDomainRecords \
  --DomainName "${DOMAIN}" --RRKeyWord "${RR}" --TypeKeyWord A --PageSize 20)"
record_id="$(jq -r --arg rr "${RR}" --arg value "${VALUE}" \
  '.DomainRecords.Record[]? | select(.RR == $rr and .Type == "A" and .Value == $value) | .RecordId' \
  <<<"${records}" | head -1)"

if [[ -n "${record_id}" && "${record_id}" != "null" ]]; then
  echo "A ${RR}.${DOMAIN} already points to ${VALUE} (RecordId=${record_id})"
  exit 0
fi

existing_id="$(jq -r --arg rr "${RR}" \
  '.DomainRecords.Record[]? | select(.RR == $rr and .Type == "A") | .RecordId' \
  <<<"${records}" | head -1)"
if [[ -n "${existing_id}" && "${existing_id}" != "null" ]]; then
  aliyun_call alidns UpdateDomainRecord \
    --RecordId "${existing_id}" --RR "${RR}" --Type A --Value "${VALUE}" --TTL "${TTL}" >/dev/null
  echo "updated A ${RR}.${DOMAIN} -> ${VALUE} (RecordId=${existing_id})"
else
  new_id="$(aliyun_call alidns AddDomainRecord \
    --DomainName "${DOMAIN}" --RR "${RR}" --Type A --Value "${VALUE}" --TTL "${TTL}" | jq -r '.RecordId')"
  [[ -n "${new_id}" && "${new_id}" != "null" ]] || { echo "DNS creation failed" >&2; exit 1; }
  echo "created A ${RR}.${DOMAIN} -> ${VALUE} (RecordId=${new_id})"
fi
