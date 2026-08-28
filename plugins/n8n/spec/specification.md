# n8n

| | |
|---|---|
| **API** | `{base_url}/api/v1` |
| **Idempotency** | No idempotency — `POST /api/v1/executions/{id}/retry` accepts no client-supplied identity and stamps none. The new execution records only `retryOf` (the source execution id), which every retry of that source shares, so `sanitize(promise.id)` can be neither injected nor recovered |
| **Reviewed by** | Claude Opus, 2026-08-27 |

**Notes**

| | |
|---|---|
| **OpenAPI** | `{base_url}/api/v1/docs/` — Swagger UI over the spec the instance itself serves (n8n Public API `1.1.1`, n8n 2.36.7); n8n publishes no standalone spec file |
| **Self-hosted** | yes — §5 |

## 1. Address

```
n8n://[{instance}]               # omitted instance = "default"
```

## 2. Configuration

```toml
[n8n.{instance}]                 # [n8n] = [n8n.default]
```

| key | type | default | example |
|---|---|---|---|
| `base_url` | `String` | | `https://n8n.acme.com` |
| `api_key` | `String` | | `…` |
| `poll` | `Duration` | `2s` | `2s` |

## 3. Authentication & Authorization

| | |
|---|---|
| **Documentation** | [API authentication](https://docs.n8n.io/connect/n8n-api/authentication) |
| **Probe** | `GET /api/v1/executions?limit=1` → `200` |

```
X-N8N-API-KEY: {api_key}
```

## 4. Operations

### 4.1 execution.retry

| | |
|---|---|
| **Documentation** | [Execution](https://docs.n8n.io/connect/n8n-api/execution) |

```json
{ "func": "execution.retry", "args": { ... } }
```

### 4.1.1 Promise Param Schema

```json
{
  "description": "Retry a failed execution and observe the execution it starts to a terminal state. Resolves when that execution reaches status \"success\"; rejects execution_failed on \"error\", cancelled on \"canceled\", crashed on \"crashed\", not_found if the source execution does not exist or the API key's user cannot reach its workflow, conflict if the source execution already succeeded, is still queued, or holds no stored execution data to resume from, invalid_request on a malformed request, and deleted if the new execution's record is removed after it reaches a terminal state and before it is read back — a running execution cannot be deleted. Statuses \"new\", \"running\", \"waiting\" (paused at a Wait node) and \"unknown\" are not terminal; \"waiting\" and \"unknown\" can persist indefinitely and are polled until timeoutAt. Duration is the workflow's own runtime — seconds to hours. The retry endpoint accepts no client-supplied identity, so a re-delivery of this promise starts a second retry of the same source execution.",
  "type": "object",
  "properties": {
    "id": {
      "type": ["integer", "string"],
      "description": "Execution to retry. Numeric; a non-numeric value is rejected. Enumerate via execution.list, which returns ids as strings."
    },
    "loadWorkflow": {
      "type": "boolean",
      "description": "Retry against the workflow's currently saved definition instead of the definition captured in the source execution. false when omitted."
    }
  },
  "required": [
    "id"
  ]
}
```

### 4.1.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "properties": {
    "id": {
      "type": "string",
      "description": "= response.body.id"
    },
    "finished": {
      "type": "boolean",
      "description": "= response.body.finished"
    },
    "mode": {
      "type": "string",
      "description": "= response.body.mode"
    },
    "status": {
      "type": "string",
      "description": "= response.body.status"
    },
    "retryOf": {
      "type": ["string", "null"],
      "description": "= response.body.retryOf (the source execution id)"
    },
    "retrySuccessId": {
      "type": ["string", "null"],
      "description": "= response.body.retrySuccessId; absent when the retry response itself carried the terminal record"
    },
    "workflowId": {
      "type": "string",
      "description": "= response.body.workflowId"
    },
    "createdAt": {
      "type": "string",
      "description": "= response.body.createdAt; absent when the retry response itself carried the terminal record"
    },
    "startedAt": {
      "type": "string",
      "description": "= response.body.startedAt"
    },
    "stoppedAt": {
      "type": ["string", "null"],
      "description": "= response.body.stoppedAt; absent when the retry response itself carried the terminal record"
    },
    "waitTill": {
      "type": ["string", "null"],
      "description": "= response.body.waitTill; absent when the retry response itself carried the terminal record"
    }
  }
}
```

#### Rejected

```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "enum": ["not_found", "conflict", "invalid_request", "execution_failed", "cancelled", "crashed", "deleted"]
    },
    "detail": {
      "description": "not_found / conflict: = response.body.message of the 404/409, or the raw body when it is not JSON; invalid_request: = response.body.message of the 400, or the raw body of a non-JSON 400 or of any other permanent 4xx; execution_failed / cancelled / crashed: = response.body (the terminal execution record); deleted: absent"
    }
  },
  "required": ["code"]
}
```

### 4.1.3 Integration Request

```
POST /api/v1/executions/{promise.param.id}/retry → 200
```

```json
{
  "type": "object",
  "properties": {
    "loadWorkflow": {
      "type": "boolean",
      "description": "= promise.param.loadWorkflow"
    }
  }
}
```

### 4.1.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "id": {
      "type": "string",
      "description": "External identity of the execution."
    },
    "finished": {
      "type": "boolean",
      "description": "Whether the run reached the end of the workflow."
    },
    "mode": {
      "type": "string",
      "enum": ["cli", "error", "integrated", "internal", "manual", "retry", "trigger", "webhook", "evaluation", "chat"],
      "description": "\"retry\" for an execution started by this endpoint."
    },
    "status": {
      "type": "string",
      "enum": ["canceled", "crashed", "error", "new", "running", "success", "unknown", "waiting"],
      "description": "success, error, canceled and crashed are terminal. new, running, waiting and unknown are not; waiting and unknown can persist indefinitely."
    },
    "retryOf": {
      "type": ["string", "number", "null"],
      "description": "Source execution id; a number in the retry response, a string in a read. null for an execution that is not a retry."
    },
    "retrySuccessId": {
      "type": ["string", "null"],
      "description": "Id of the retry of this execution that succeeded. Absent from the retry response."
    },
    "workflowId": {
      "type": "string"
    },
    "workflowVersionId": {
      "type": ["string", "null"],
      "description": "Absent from the retry response."
    },
    "createdAt": {
      "type": "string",
      "description": "ISO 8601. Absent from the retry response."
    },
    "startedAt": {
      "type": "string",
      "description": "ISO 8601"
    },
    "stoppedAt": {
      "type": ["string", "null"],
      "description": "ISO 8601; null while the status is running. Absent from the retry response."
    },
    "deletedAt": {
      "type": ["string", "null"],
      "description": "ISO 8601. Absent from the retry response."
    },
    "waitTill": {
      "type": ["string", "null"],
      "description": "ISO 8601; when the status is waiting, the time the run resumes. Absent from the retry response."
    },
    "storedAt": {
      "type": "string",
      "description": "Where the execution data lives, e.g. \"db\"."
    },
    "deduplicationKey": {
      "type": ["string", "null"],
      "description": "Set by n8n for scheduled triggers; not settable through this API. Absent from the retry response."
    },
    "jsonSizeBytes": {
      "type": ["number", "null"],
      "description": "Absent from the retry response."
    },
    "binaryDataSizeBytes": {
      "type": ["number", "null"],
      "description": "Absent from the retry response."
    },
    "usedPrivateCredentials": {
      "type": "boolean",
      "description": "Absent from the retry response."
    },
    "tracingContext": {
      "type": ["object", "null"],
      "description": "Absent from the retry response.",
      "additionalProperties": true
    },
    "customData": {
      "type": "object",
      "description": "Present in the retry response and in reads made with includeData.",
      "additionalProperties": true
    },
    "data": {
      "type": "object",
      "description": "Node run data of the execution: version, startData, resultData, executionData, resumeToken. Present in the retry response and in reads made with includeData. redactionInfo is present when the data was redacted.",
      "additionalProperties": true
    },
    "workflowData": {
      "type": "object",
      "description": "The workflow definition the execution ran. Present in the retry response and in reads made with includeData.",
      "additionalProperties": true
    },
    "message": {
      "type": "string",
      "description": "Error text; present instead of the record on a 4xx."
    }
  },
  "required": [
    "id",
    "finished",
    "mode",
    "status",
    "retryOf",
    "workflowId",
    "startedAt"
  ]
}
```

### 4.1.5 Implementation

| | |
|---|---|
| **Invocation** | `create` |
| **Monitoring** | `request_poll` |

```python
import time
from urllib.parse import quote

import requests

TERMINAL = ("success", "error", "canceled", "crashed")


def _check(r):
    # 401: the API key is absent, malformed, or expired. 403: the key lacks
    # the endpoint's scope. Both need an operator.
    if r.status_code in (401, 403):
        raise Exception("halt", r.text)
    # n8n documents no rate-limit status; a 429 or 5xx comes from the server
    # itself or from whatever fronts it.
    if r.status_code == 429 or r.status_code >= 500:
        raise Exception("release", r.text)
    return r


def _api(cfg):
    return f"{cfg.base_url}/api/v1"


def _headers(cfg):
    return {"X-N8N-API-KEY": cfg.api_key}


def _message(r):
    # Anything fronting a self-hosted base_url can answer 4xx without n8n's
    # JSON body.
    try:
        return r.json()["message"]
    except Exception:
        return r.text


def _query(args, keys=None):
    # n8n's request validator answers 400 "must be boolean" for Python's
    # True/False rendering; the wire form is "true"/"false". Cursors are
    # passed through byte for byte — requests does the percent-encoding.
    return {
        k: ("true" if v is True else "false" if v is False else v)
        for k, v in args.items()
        if keys is None or k in keys
    }


def _find_retry(cfg, source_id):
    # A retried execution carries retryOf = the source execution id, and its
    # row exists before the retry endpoint starts blocking. GET /executions
    # returns newest first, caps limit at 250, and omits running executions
    # unless status=running is set, so both listings are scanned.
    for params in ({"status": "running", "limit": 250}, {"limit": 250}):
        r = _check(
            requests.get(
                f"{_api(cfg)}/executions",
                headers=_headers(cfg),
                params=params,
                timeout=10,
            )
        )
        if r.status_code >= 400:
            raise Exception("release", r.text)
        for e in r.json()["data"]:
            if e["retryOf"] is not None and str(e["retryOf"]) == source_id:
                return quote(str(e["id"]), safe="")
    return None


def execution_retry(cfg, promise):
    args = promise.param["args"]
    source_id = quote(str(args["id"]), safe="")
    body = {}
    if "loadWorkflow" in args:
        body["loadWorkflow"] = args["loadWorkflow"]

    # Unkeyed POST: a re-delivery retries the source execution again, costing
    # one more run of the workflow.
    try:
        r = requests.post(
            f"{_api(cfg)}/executions/{source_id}/retry",
            headers=_headers(cfg),
            json=body,
            timeout=10,
        )
    except requests.exceptions.ReadTimeout:
        # The endpoint withholds its response until the execution it started
        # settles, so a workflow that runs longer than this timeout never
        # answers here. That execution is unaffected and is recovered below.
        r = None
    if r is not None and r.status_code >= 500:
        # The endpoint answers 500 when the execution it started was
        # cancelled, so a 5xx is not evidence that nothing ran. The started
        # execution is recovered below; only its absence is a release.
        r = None
    run = None
    execution_id = None
    if r is not None:
        if r.status_code == 404:
            return ("rejected", {"code": "not_found", "detail": _message(r)})
        if r.status_code == 409:
            # The source execution succeeded, is still queued, or was aborted
            # before any node data was stored, so there is nothing to resume.
            return ("rejected", {"code": "conflict", "detail": _message(r)})
        if r.status_code == 400:
            return ("rejected", {"code": "invalid_request", "detail": _message(r)})
        _check(r)
        if r.status_code >= 400:
            return ("rejected", {"code": "invalid_request", "detail": r.text})
        # The response body is the execution as it settled.
        run = r.json()
        execution_id = quote(str(run["id"]), safe="")
    if execution_id is None:
        execution_id = _find_retry(cfg, str(args["id"]))
    if execution_id is None:
        raise Exception("release", "retried execution not found")

    failures = 0
    while run is None or run["status"] not in TERMINAL:
        if time.time() * 1000 >= promise.timeout_at:
            raise Exception("release", "promise timed out")
        time.sleep(
            min(
                cfg.poll.total_seconds(),
                (promise.timeout_at - time.time() * 1000) / 1000,
            )
        )
        try:
            g = _check(
                requests.get(
                    f"{_api(cfg)}/executions/{execution_id}",
                    headers=_headers(cfg),
                    timeout=10,
                )
            )
        except Exception as e:
            if e.args[:1] == ("halt",):
                raise
            # The retried execution's id is unrecoverable on re-entry —
            # absorb, bounded.
            failures += 1
            if failures >= 5:
                raise
            continue
        failures = 0
        if g.status_code == 404:
            # A terminal execution's record was removed; n8n refuses to delete
            # a running one.
            return ("rejected", {"code": "deleted"})
        if g.status_code >= 400:
            return ("rejected", {"code": "invalid_request", "detail": g.text})
        run = g.json()

    if run["status"] == "success":
        keys = (
            "id",
            "finished",
            "mode",
            "status",
            "retryOf",
            "retrySuccessId",
            "workflowId",
            "createdAt",
            "startedAt",
            "stoppedAt",
            "waitTill",
        )
        # The 4.1.2 Resolved mapping; the retry response omits createdAt,
        # stoppedAt, waitTill and retrySuccessId.
        return ("resolved", {k: run[k] for k in keys if k in run})
    if run["status"] == "canceled":
        return ("rejected", {"code": "cancelled", "detail": run})
    if run["status"] == "crashed":
        return ("rejected", {"code": "crashed", "detail": run})
    return ("rejected", {"code": "execution_failed", "detail": run})
```


### 4.2 execution.get

| | |
|---|---|
| **Documentation** | [Execution](https://docs.n8n.io/connect/n8n-api/execution) |

```json
{ "func": "execution.get", "args": { ... } }
```

### 4.2.1 Promise Param Schema

```json
{
  "description": "Read one execution, optionally with the node run data it produced — the result a workflow run published. Rejects not_found if the execution does not exist or the API key's user cannot reach its workflow, invalid_request on a malformed request. A plain read — not the completion mechanism; execution.retry observes independently.",
  "type": "object",
  "properties": {
    "id": {
      "type": ["integer", "string"],
      "description": "Execution id. Numeric; a non-numeric value is rejected."
    },
    "includeData": {
      "type": "boolean",
      "description": "Include the execution's data and workflowData. false when omitted."
    },
    "ignoreDataSizeLimit": {
      "type": "boolean",
      "description": "Return the data even when it exceeds EXECUTIONS_DATA_MAX_DISPLAY_SIZE; an oversized execution is otherwise returned without it. false when omitted."
    },
    "redactExecutionData": {
      "type": "boolean",
      "description": "true always redacts the output data; false requests unredacted data and needs the execution:reveal scope. The workflow's redaction policy applies when omitted."
    }
  },
  "required": [
    "id"
  ],
  "additionalProperties": false
}
```

### 4.2.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "description": "= response.body"
}
```

#### Rejected

```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "enum": ["not_found", "invalid_request"]
    },
    "detail": {
      "description": "invalid_request: = response.body.message of the 400, or the raw body of a non-JSON 400 or of any other permanent 4xx; not_found: absent"
    }
  },
  "required": ["code"]
}
```

### 4.2.3 Integration Request

```
GET /api/v1/executions/{promise.param.id}{?includeData,ignoreDataSizeLimit,redactExecutionData = promise.param.*} → 200
```

### 4.2.4 Integration Response

Same as 4.1.4.

### 4.2.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def execution_get(cfg, promise):
    args = promise.param["args"]
    execution_id = quote(str(args["id"]), safe="")

    keys = ("includeData", "ignoreDataSizeLimit", "redactExecutionData")
    r = requests.get(
        f"{_api(cfg)}/executions/{execution_id}",
        headers=_headers(cfg),
        params=_query(args, keys),
        timeout=10,
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found"})
    if r.status_code == 400:
        return ("rejected", {"code": "invalid_request", "detail": _message(r)})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    return ("resolved", r.json())
```


### 4.3 execution.list

| | |
|---|---|
| **Documentation** | [Execution](https://docs.n8n.io/connect/n8n-api/execution) |

```json
{ "func": "execution.list", "args": { ... } }
```

### 4.3.1 Promise Param Schema

```json
{
  "description": "List executions, for finding the failed execution that execution.retry resumes. Running executions are omitted unless status is \"running\". Results are newest first, one page per call. An unreachable or unknown workflowId or projectId yields an empty page. Rejects invalid_request on a malformed or unknown filter.",
  "type": "object",
  "properties": {
    "status": {
      "type": "string",
      "enum": ["canceled", "crashed", "error", "new", "running", "success", "unknown", "waiting"],
      "description": "Restrict to executions in this status."
    },
    "workflowId": {
      "type": "string",
      "description": "Restrict to executions of this workflow. An unknown id yields an empty page, not an error."
    },
    "projectId": {
      "type": "string",
      "description": "Restrict to executions of workflows in this project. An unknown id yields an empty page, not an error."
    },
    "includeData": {
      "type": "boolean",
      "description": "Include each execution's data and workflowData. false when omitted."
    },
    "ignoreDataSizeLimit": {
      "type": "boolean",
      "description": "Return the data even when it exceeds EXECUTIONS_DATA_MAX_DISPLAY_SIZE; an oversized execution is otherwise returned without it. false when omitted."
    },
    "redactExecutionData": {
      "type": "boolean",
      "description": "true always redacts the output data; false requests unredacted data and needs the execution:reveal scope. The workflow's redaction policy applies when omitted."
    },
    "limit": {
      "type": "integer",
      "maximum": 250,
      "description": "Page size; 100 when omitted."
    },
    "cursor": {
      "type": "string",
      "description": "The nextCursor of the previous page, passed through unmodified. The first page when omitted."
    }
  },
  "additionalProperties": false
}
```

### 4.3.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "description": "= response.body"
}
```

#### Rejected

```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "enum": ["invalid_request"]
    },
    "detail": {
      "description": "invalid_request: = response.body.message of the 400, or the raw body of a non-JSON 400 or of any other permanent 4xx"
    }
  },
  "required": ["code"]
}
```

### 4.3.3 Integration Request

```
GET /api/v1/executions{?status,workflowId,projectId,includeData,ignoreDataSizeLimit,redactExecutionData,limit,cursor = promise.param.*} → 200
```

### 4.3.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "data": {
      "type": "array",
      "items": {
        "description": "Same as 4.1.4"
      }
    },
    "nextCursor": {
      "type": ["string", "null"],
      "description": "Cursor for the next page; null on the last page."
    }
  },
  "required": [
    "data",
    "nextCursor"
  ]
}
```

### 4.3.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def execution_list(cfg, promise):
    args = promise.param["args"]

    # Pagination is the caller's loop (one promise per page), not ours.
    r = requests.get(
        f"{_api(cfg)}/executions",
        headers=_headers(cfg),
        params=_query(args),
        timeout=10,
    )
    if r.status_code == 400:
        return ("rejected", {"code": "invalid_request", "detail": _message(r)})
    _check(r)
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    return ("resolved", r.json())
```


## 5. Test

### 5.1 Base Image

```dockerfile
FROM python:3.13-alpine AS python
RUN pip install --no-cache-dir requests

FROM n8nio/n8n:2.36.7
USER root
COPY --from=python /usr/local/bin/python3.13 /usr/local/bin/python3.13
COPY --from=python /usr/local/lib/libpython3.13.so.1.0 /usr/local/lib/
COPY --from=python /usr/local/lib/python3.13 /usr/local/lib/python3.13
COPY --from=python /usr/lib/libffi.so.8* /usr/lib/
RUN ln -sf /usr/local/bin/python3.13 /usr/local/bin/python3
USER node
ENV N8N_LISTEN_ADDRESS=0.0.0.0 \
    N8N_ENCRYPTION_KEY=resonate-plugin-test \
    N8N_DIAGNOSTICS_ENABLED=false \
    N8N_PERSONALIZATION_ENABLED=false \
    N8N_SECURE_COOKIE=false \
    N8N_RUNNERS_ENABLED=true \
    GENERIC_TIMEZONE=UTC
```

### 5.2 Run

```sh
docker rm -f plugin-n8n-test >/dev/null 2>&1 || true
docker build -t plugin-n8n-test spec/
docker run -d --name plugin-n8n-test -p 5678:5678 plugin-n8n-test

N8N_BASE_URL=http://localhost:5678
COOKIES=$(mktemp)
OWNER='{"email":"resonate@example.com","firstName":"Res","lastName":"Onate","password":"Resonate123"}'
# /rest answers 200 with a "starting up" placeholder until n8n is really up,
# so the gate is the owner record coming back, not the status code.
until curl -s -c "$COOKIES" -X POST "$N8N_BASE_URL/rest/owner/setup" \
        -H 'Content-Type: application/json' -d "$OWNER" \
      | python3 -c 'import json,sys; sys.exit(0 if json.load(sys.stdin)["data"]["id"] else 1)' 2>/dev/null; do
  sleep 5
done

SCOPES='["workflow:create","workflow:activate","workflow:list","workflow:read","execution:list","execution:read","execution:retry"]'
N8N_API_KEY=$(curl -s -b "$COOKIES" -X POST "$N8N_BASE_URL/rest/api-keys" \
  -H 'Content-Type: application/json' \
  -d "{\"label\":\"resonate\",\"expiresAt\":null,\"scopes\":$SCOPES}" \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["data"]["rawApiKey"])')
until curl -sf -o /dev/null -H "X-N8N-API-KEY: $N8N_API_KEY" \
        "$N8N_BASE_URL/api/v1/executions?limit=1"; do sleep 5; done

api() { curl -sf -H "X-N8N-API-KEY: $N8N_API_KEY" -H 'Content-Type: application/json' "$@"; }
wf_id() { python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])'; }
hook() { printf '{"parameters":{"httpMethod":"POST","path":"%s","options":{}},"name":"Webhook","type":"n8n-nodes-base.webhook","typeVersion":2,"position":[0,0]}' "$1"; }

# resonate-gate stays inactive for now: while it is inactive its webhook path
# 404s, which is what makes the FIXTURE_OK run fail.
GATE=$(api -X POST "$N8N_BASE_URL/api/v1/workflows" \
  -d "{\"name\":\"resonate-gate\",\"nodes\":[$(hook resonate-gate)],\"connections\":{},\"settings\":{\"executionOrder\":\"v1\"}}" | wf_id)
OK_WF=$(api -X POST "$N8N_BASE_URL/api/v1/workflows" \
  -d "{\"name\":\"resonate-ok\",\"nodes\":[$(hook resonate-ok),{\"parameters\":{\"method\":\"POST\",\"url\":\"http://localhost:5678/webhook/resonate-gate\",\"options\":{}},\"name\":\"HTTP Request\",\"type\":\"n8n-nodes-base.httpRequest\",\"typeVersion\":4.2,\"position\":[220,0]}],\"connections\":{\"Webhook\":{\"main\":[[{\"node\":\"HTTP Request\",\"type\":\"main\",\"index\":0}]]}},\"settings\":{\"executionOrder\":\"v1\"}}" | wf_id)
FAIL_WF=$(api -X POST "$N8N_BASE_URL/api/v1/workflows" \
  -d "{\"name\":\"resonate-fail\",\"nodes\":[$(hook resonate-fail),{\"parameters\":{\"jsCode\":\"throw new Error('resonate fixture always fails');\"},\"name\":\"Code\",\"type\":\"n8n-nodes-base.code\",\"typeVersion\":2,\"position\":[220,0]}],\"connections\":{\"Webhook\":{\"main\":[[{\"node\":\"Code\",\"type\":\"main\",\"index\":0}]]}},\"settings\":{\"executionOrder\":\"v1\"}}" | wf_id)

api -X POST "$N8N_BASE_URL/api/v1/workflows/$OK_WF/activate" >/dev/null
api -X POST "$N8N_BASE_URL/api/v1/workflows/$FAIL_WF/activate" >/dev/null

failed_execution() {
  until curl -sf -o /dev/null -X POST "$N8N_BASE_URL/webhook/$1" \
          -H 'Content-Type: application/json' -d '{}'; do sleep 2; done
  until EXEC=$(api "$N8N_BASE_URL/api/v1/executions?workflowId=$2&status=error&limit=1" \
                 | python3 -c 'import json,sys; d=json.load(sys.stdin)["data"]; print(d[0]["id"] if d else "")') \
        && [ -n "$EXEC" ]; do sleep 2; done
  echo "$EXEC"
}
N8N_FIXTURE_OK=$(failed_execution resonate-ok "$OK_WF")
N8N_FIXTURE_FAIL=$(failed_execution resonate-fail "$FAIL_WF")

# Activating the gate makes every later retry of FIXTURE_OK succeed;
# resonate-fail throws unconditionally, so its retries always fail.
api -X POST "$N8N_BASE_URL/api/v1/workflows/$GATE/activate" >/dev/null
until curl -sf -o /dev/null -X POST "$N8N_BASE_URL/webhook/resonate-gate" -d '{}'; do sleep 2; done

export N8N_BASE_URL
export N8N_API_KEY
export N8N_FIXTURE_OK
export N8N_FIXTURE_FAIL
```
