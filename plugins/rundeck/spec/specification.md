# Rundeck

| | |
|---|---|
| **API** | `{base_url}/api/59` |
| **Idempotency** | No idempotency key. `job.run`, `job.submit` and `job.retry` stamp the execution instead: `sanitize(promise.id)` is sent as the value of a job option named `rescorr`, which Rundeck stores verbatim even when the job does not declare it — it appears in the execution's `argstring` and in its parsed `job.options` — and `GET /api/59/project/[PROJECT]/executions?optionFilter=-rescorr <value>` locates it by partial match, with no expiry window. Uniqueness is per project, so the match is re-checked exactly against `job.options.rescorr`. Option values are free text: `sanitize`'s 17–117 characters of `[A-Za-z0-9._-]` pass through unchanged, a leading `-` included. A job that itself declares an option named `rescorr` collides |
| **Reviewed by** | Claude Opus 5, 2026-08-31 |

**Notes**

| | |
|---|---|
| **OpenAPI** | `https://docs.rundeck.com/docs/files/rundeck-api.yml` — unreachable from here (the egress proxy denies CONNECT, 403); read instead from the docs repository that publishes it, `https://raw.githubusercontent.com/rundeck/docs/6.2.0/docs/.vuepress/public/files/rundeck-api.yml` (`openapi: 3.0.1`, `info.version: "59"`), which `docs/api/api-spec.md` embeds as `<rundeck-swagger-ui specFile="/files/rundeck-api.yml"/>`. The API Reference behind `https://docs.rundeck.com/docs/api/` is unreachable for the same reason and was read as `https://raw.githubusercontent.com/rundeck/docs/6.2.0/docs/api/index.md`, the page's own source, which every Documentation row below links to |
| **Self-hosted** | yes — §5 |

## 1. Address

```
rundeck://[{instance}]           # omitted instance = "default"
```

## 2. Configuration

```toml
[rundeck.{instance}]             # [rundeck] = [rundeck.default]
```

| key | type | default | example |
|---|---|---|---|
| `base_url` | `String` | | `https://rundeck.acme.com` |
| `api_token` | `String` | | `E4rNvVRV378knO9dp3d73O0cs1kd0kCd` |
| `poll` | `Duration` | `5s` | `5s` |
| `poll_job_run` | `Option<Duration>` | `= poll` | `10s` |
| `poll_job_retry` | `Option<Duration>` | `= poll` | `10s` |
| `poll_execution_abort` | `Option<Duration>` | `= poll` | `2s` |

## 3. Authentication & Authorization

| | |
|---|---|
| **Documentation** | [API Reference — Authentication](https://raw.githubusercontent.com/rundeck/docs/6.2.0/docs/api/index.md) |
| **Probe** | `GET /api/59/projects` → `200` |

```
X-Rundeck-Auth-Token: {api_token}
```

## 4. Operations

### 4.1 job.run

| | |
|---|---|
| **Documentation** | [API Reference — Running a Job](https://raw.githubusercontent.com/rundeck/docs/6.2.0/docs/api/index.md) |

```json
{ "func": "job.run", "args": { ... } }
```

### 4.1.1 Promise Param Schema

```json
{
  "description": "Run a saved job and observe its execution to a terminal state. Resolves when the execution reaches status \"succeeded\"; rejects execution_failed, execution_failed_with_retry, execution_aborted, execution_timedout or execution_other on the other terminal statuses, job_not_found if the job UUID is unknown, invalid_request if Rundeck refuses the request (option validation, executions disabled for the job), and execution_deleted if the execution is removed before it finishes. Duration is the job's own runtime — seconds to hours; runAtTime parks the execution in the non-terminal status \"scheduled\" until that time and it stays there, so size timeoutAt to cover the delay as well as the run. Adhoc command, script and script-URL dispatch (POST /api/59/project/[PROJECT]/run/command, /run/script, /run/url) is not exposed: it creates an execution without a saved job, and a workflow that wants one should define a job. File-type job options are not exposed either: their values are file keys obtained from POST /api/19/job/[ID]/input/file, which only jobs declaring a file option can use.",
  "type": "object",
  "properties": {
    "id": {
      "type": "string",
      "description": "Job UUID. Enumerate via job.list."
    },
    "options": {
      "type": "object",
      "description": "Option name to value. Overrides argString when both are given. The job's declared options and their defaults are in job.get.",
      "additionalProperties": true
    },
    "argString": {
      "type": "string",
      "description": "Option values in \"-opt value -opt2 value\" form. Ignored by Rundeck when options is also given."
    },
    "loglevel": {
      "type": "string",
      "enum": ["DEBUG", "VERBOSE", "INFO", "WARN", "ERROR"]
    },
    "asUser": {
      "type": "string",
      "description": "Username to record as the one who ran the job. Requires runAs authorization."
    },
    "filter": {
      "type": "string",
      "description": "Node filter string restricting the target nodes."
    },
    "runAtTime": {
      "type": "string",
      "description": "ISO-8601 date and time with timezone and optional milliseconds, e.g. \"2016-11-23T12:20:55-0800\". The execution is created in status \"scheduled\" and runs then."
    }
  },
  "required": ["id"]
}
```

### 4.1.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "properties": {
    "id": { "type": "integer", "description": "= response.body.id" },
    "href": { "type": "string", "description": "= response.body.href" },
    "permalink": { "type": "string", "description": "= response.body.permalink" },
    "status": { "type": "string", "description": "= response.body.status" },
    "customStatus": { "type": ["string", "null"], "description": "= response.body.customStatus" },
    "project": { "type": "string", "description": "= response.body.project" },
    "executionType": { "type": "string", "description": "= response.body.executionType" },
    "user": { "type": "string", "description": "= response.body.user" },
    "date-started": { "type": "object", "description": "= response.body.date-started" },
    "date-ended": { "type": ["object", "null"], "description": "= response.body.date-ended" },
    "job": { "type": "object", "description": "= response.body.job" },
    "description": { "type": "string", "description": "= response.body.description" },
    "argstring": { "type": ["string", "null"], "description": "= response.body.argstring" },
    "successfulNodes": { "type": ["array", "null"], "description": "= response.body.successfulNodes" },
    "failedNodes": { "type": ["array", "null"], "description": "= response.body.failedNodes" },
    "retriedExecution": { "type": ["object", "null"], "description": "= response.body.retriedExecution" }
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
      "enum": [
        "job_not_found",
        "invalid_request",
        "execution_deleted",
        "execution_failed",
        "execution_failed_with_retry",
        "execution_aborted",
        "execution_timedout",
        "execution_other"
      ]
    },
    "detail": {
      "description": "job_not_found: = response.body.message of the 404; invalid_request: the error body; execution_failed / execution_failed_with_retry / execution_aborted / execution_timedout / execution_other: = response.body (the terminal execution record); execution_deleted: absent"
    }
  },
  "required": ["code"]
}
```

### 4.1.3 Integration Request

```
POST /api/59/job/{promise.param.id}/run → 200
Content-Type: application/json
```

```json
{
  "type": "object",
  "properties": {
    "options": {
      "type": "object",
      "description": "= promise.param.options, plus rescorr = sanitize(promise.id); sent when the caller supplied options",
      "additionalProperties": true
    },
    "argString": {
      "type": "string",
      "description": "= promise.param.argString with \" -rescorr \" + sanitize(promise.id) appended; sent when the caller supplied no options"
    },
    "loglevel": {
      "type": "string",
      "description": "= promise.param.loglevel"
    },
    "asUser": {
      "type": "string",
      "description": "= promise.param.asUser"
    },
    "filter": {
      "type": "string",
      "description": "= promise.param.filter"
    },
    "runAtTime": {
      "type": "string",
      "description": "= promise.param.runAtTime"
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
      "type": "integer",
      "description": "External identity of the execution."
    },
    "href": { "type": "string" },
    "permalink": { "type": "string" },
    "status": {
      "type": "string",
      "enum": ["running", "scheduled", "succeeded", "failed", "failed-with-retry", "aborted", "timedout", "other"],
      "description": "succeeded, failed, failed-with-retry, aborted, timedout and other are terminal. running and scheduled are not: an execution created with runAtTime stays scheduled until that time. failed-with-retry means this execution is over and Rundeck started a separate retry execution, named by retriedExecution."
    },
    "customStatus": {
      "type": "string",
      "description": "The custom exit status string; present when status is other."
    },
    "project": { "type": "string" },
    "executionType": {
      "type": "string",
      "enum": ["scheduled", "user", "user-scheduled"]
    },
    "user": {
      "type": "string",
      "description": "Username that started the execution."
    },
    "abortedby": {
      "type": "string",
      "description": "Username that aborted the execution; present once an abort took effect."
    },
    "serverUUID": {
      "type": "string",
      "description": "Cluster server that ran the execution."
    },
    "date-started": {
      "type": "object",
      "properties": {
        "unixtime": { "type": "integer", "description": "Milliseconds since epoch." },
        "date": { "type": "string", "description": "W3C dateTime, yyyy-MM-ddTHH:mm:ssZ." }
      }
    },
    "date-ended": {
      "type": "object",
      "description": "Same shape as date-started; absent until the execution is terminal."
    },
    "job": {
      "type": "object",
      "description": "Absent for adhoc executions.",
      "properties": {
        "id": { "type": "string", "description": "Job UUID." },
        "name": { "type": "string" },
        "group": { "type": ["string", "null"] },
        "project": { "type": "string" },
        "description": { "type": "string" },
        "averageDuration": { "type": "integer", "description": "Milliseconds." },
        "href": { "type": "string" },
        "permalink": { "type": "string" },
        "options": {
          "type": "object",
          "description": "The parsed option name to value map, present when the execution has an argstring. Carries the rescorr stamp of 4.1.3.",
          "additionalProperties": true
        }
      }
    },
    "description": { "type": "string" },
    "argstring": {
      "type": ["string", "null"],
      "description": "The option values in \"-opt value\" form; null when the execution has none."
    },
    "successfulNodes": {
      "type": "array",
      "items": { "type": "string" }
    },
    "failedNodes": {
      "type": "array",
      "items": { "type": "string" }
    },
    "retriedExecution": {
      "type": "object",
      "description": "Present when status is failed-with-retry: the separate execution Rundeck started to retry this one.",
      "properties": {
        "id": { "type": "integer" },
        "href": { "type": "string" },
        "permalink": { "type": "string" },
        "status": { "type": "string" }
      }
    },
    "retryAttempt": {
      "type": "integer",
      "description": "Present on an execution that is itself a retry; 1 for the first."
    },
    "jobDeleted": { "type": "boolean" }
  },
  "required": ["id", "href", "permalink", "status", "project", "user", "date-started"]
}
```

### 4.1.5 Implementation

| | |
|---|---|
| **Algebra** | `call + poll` |
| **Invocation** | `fetch_then_create` |
| **Monitoring** | `request_poll` |

```python
import time
from urllib.parse import quote

import requests

CORR = "rescorr"
TERMINAL = ("succeeded", "failed", "failed-with-retry", "aborted", "timedout", "other")
RESULT = (
    "id",
    "href",
    "permalink",
    "status",
    "customStatus",
    "project",
    "executionType",
    "user",
    "date-started",
    "date-ended",
    "job",
    "description",
    "argstring",
    "successfulNodes",
    "failedNodes",
    "retriedExecution",
)


def _check(r):
    # 403 with errorCode "unauthorized" is the only authorization status
    # Rundeck returns: an absent, unknown or under-privileged token. It ends
    # when an operator issues a token or grants the ACL, not before.
    if r.status_code == 403:
        raise Exception("halt", r.text)
    if r.status_code == 429 or r.status_code >= 500:
        raise Exception("release", r.text)
    return r


def _auth(cfg):
    return {"X-Rundeck-Auth-Token": cfg.api_token}


def _query(args, keys):
    params = {}
    for k in keys:
        v = args.get(k)
        if v is None:
            continue
        # Rundeck's list filters are repeated keys, which requests renders
        # from a list value; booleans go on the wire as true/false.
        params[k] = "true" if v is True else "false" if v is False else v
    return params


def _stamp(args, token, keys):
    body = {k: args[k] for k in keys if k in args}
    if args.get("options") is not None:
        # An options map makes Rundeck ignore argString entirely, so the
        # stamp joins the map the caller supplied.
        body["options"] = dict(args["options"], **{CORR: token})
    else:
        # Undeclared options survive: Rundeck parses the whole argString into
        # the execution's job.options and applies declared defaults anyway.
        base = args.get("argString") or ""
        body["argString"] = f"{base} -{CORR} {token}".strip()
    return body


def _find(cfg, project, token):
    proj = quote(project, safe="")
    r = _check(
        requests.get(
            f"{cfg.base_url}/api/59/project/{proj}/executions",
            headers=_auth(cfg),
            params={"optionFilter": f"-{CORR} {token}", "max": 20},
            timeout=10,
        )
    )
    if r.status_code >= 400:
        return None
    # optionFilter is a partial match over the option values, so confirm the
    # stamp exactly.
    for e in r.json()["executions"]:
        if ((e.get("job") or {}).get("options") or {}).get(CORR) == token:
            return e
    return None


def _settle(execution):
    status = execution["status"]
    if status == "succeeded":
        return ("resolved", {k: execution.get(k) for k in RESULT})
    if status == "failed":
        return ("rejected", {"code": "execution_failed", "detail": execution})
    if status == "failed-with-retry":
        # This execution is over; the retry Rundeck started is a different
        # execution, named by retriedExecution.
        return ("rejected", {"code": "execution_failed_with_retry", "detail": execution})
    if status == "aborted":
        return ("rejected", {"code": "execution_aborted", "detail": execution})
    if status == "timedout":
        return ("rejected", {"code": "execution_timedout", "detail": execution})
    # "other": a custom exit status, its string in customStatus.
    return ("rejected", {"code": "execution_other", "detail": execution})


def _await(cfg, exec_id, cadence, promise):
    eid = quote(str(exec_id), safe="")
    while True:
        if time.time() * 1000 >= promise.timeout_at:
            raise Exception("release", "promise timed out")
        r = _check(
            requests.get(
                f"{cfg.base_url}/api/59/execution/{eid}",
                headers=_auth(cfg),
                timeout=10,
            )
        )
        if r.status_code == 404:
            return None
        if r.status_code >= 400:
            raise Exception("release", r.text)
        execution = r.json()
        if execution["status"] in TERMINAL:
            return execution
        # "scheduled" is not terminal and holds until runAtTime.
        time.sleep(
            min(cadence, (promise.timeout_at - time.time() * 1000) / 1000)
        )


def job_run(cfg, promise):
    args = promise.param["args"]
    job_id = quote(args["id"], safe="")
    token = sanitize(promise.id)

    # The stamp is only searchable within a project, and /job/[ID]/info is
    # the cheapest read that names one. It answers XML unless asked for JSON.
    r = _check(
        requests.get(
            f"{cfg.base_url}/api/59/job/{job_id}/info",
            headers=_auth(cfg) | {"Accept": "application/json"},
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "job_not_found", "detail": r.json()["message"]})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})

    execution = _find(cfg, r.json()["project"], token)
    if execution is None:
        r = _check(
            requests.post(
                f"{cfg.base_url}/api/59/job/{job_id}/run",
                headers=_auth(cfg) | {"Content-Type": "application/json"},
                json=_stamp(args, token, ("loglevel", "asUser", "filter", "runAtTime")),
                timeout=10,
            )
        )
        if r.status_code == 409:
            # api.error.execution.conflict: the job's own limit on concurrent
            # executions. It clears when the execution now running ends.
            raise Exception("release", r.text)
        if r.status_code == 404:
            return ("rejected", {"code": "job_not_found", "detail": r.json()["message"]})
        if r.status_code >= 400:
            return ("rejected", {"code": "invalid_request", "detail": r.text})
        execution = r.json()

    execution = _await(cfg, execution["id"], cfg.poll_job_run.total_seconds(), promise)
    if execution is None:
        return ("rejected", {"code": "execution_deleted"})
    return _settle(execution)
```

### 4.2 job.submit

| | |
|---|---|
| **Documentation** | [API Reference — Running a Job](https://raw.githubusercontent.com/rundeck/docs/6.2.0/docs/api/index.md) |

```json
{ "func": "job.submit", "args": { ... } }
```

### 4.2.1 Promise Param Schema

```json
{
  "description": "Hand a saved job to Rundeck and resolve as soon as the execution is accepted. Resolution means accepted, not finished: the promise carries the execution handle, whose status is \"running\" or \"scheduled\" when this delivery created it, and whatever status it has since reached when a redelivery re-found an execution this promise already created. Await the outcome with job.run instead, or come back to the handle later with execution.get, execution.output or execution.state — an execution record is readable until someone deletes it. Rejects job_not_found if the job UUID is unknown and invalid_request if Rundeck refuses the request.",
  "type": "object",
  "properties": {
    "id": {
      "type": "string",
      "description": "Job UUID. Enumerate via job.list."
    },
    "options": {
      "type": "object",
      "description": "Option name to value. Overrides argString when both are given. The job's declared options and their defaults are in job.get.",
      "additionalProperties": true
    },
    "argString": {
      "type": "string",
      "description": "Option values in \"-opt value -opt2 value\" form. Ignored by Rundeck when options is also given."
    },
    "loglevel": {
      "type": "string",
      "enum": ["DEBUG", "VERBOSE", "INFO", "WARN", "ERROR"]
    },
    "asUser": {
      "type": "string",
      "description": "Username to record as the one who ran the job. Requires runAs authorization."
    },
    "filter": {
      "type": "string",
      "description": "Node filter string restricting the target nodes."
    },
    "runAtTime": {
      "type": "string",
      "description": "ISO-8601 date and time with timezone and optional milliseconds, e.g. \"2016-11-23T12:20:55-0800\". The execution is created in status \"scheduled\" and runs then."
    }
  },
  "required": ["id"]
}
```

### 4.2.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "description": "= response.body — the accepted execution record of 4.1.4; status is running or scheduled on the delivery that creates it, and may already be terminal when a redelivery re-finds it"
}
```

#### Rejected

```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "enum": ["job_not_found", "invalid_request"]
    },
    "detail": {
      "description": "job_not_found: = response.body.message of the 404; invalid_request: the error body"
    }
  },
  "required": ["code"]
}
```

### 4.2.3 Integration Request

```
POST /api/59/job/{promise.param.id}/run → 200
Content-Type: application/json
```

```json
{
  "type": "object",
  "properties": {
    "options": {
      "type": "object",
      "description": "= promise.param.options, plus rescorr = sanitize(promise.id); sent when the caller supplied options",
      "additionalProperties": true
    },
    "argString": {
      "type": "string",
      "description": "= promise.param.argString with \" -rescorr \" + sanitize(promise.id) appended; sent when the caller supplied no options"
    },
    "loglevel": {
      "type": "string",
      "description": "= promise.param.loglevel"
    },
    "asUser": {
      "type": "string",
      "description": "= promise.param.asUser"
    },
    "filter": {
      "type": "string",
      "description": "= promise.param.filter"
    },
    "runAtTime": {
      "type": "string",
      "description": "= promise.param.runAtTime"
    }
  }
}
```

### 4.2.4 Integration Response

Same as 4.1.4.

### 4.2.5 Implementation

| | |
|---|---|
| **Algebra** | `call` |
| **Invocation** | `fetch_then_create` |
| **Monitoring** | `request_response` |

```python
def job_submit(cfg, promise):
    args = promise.param["args"]
    job_id = quote(args["id"], safe="")
    token = sanitize(promise.id)

    r = _check(
        requests.get(
            f"{cfg.base_url}/api/59/job/{job_id}/info",
            headers=_auth(cfg) | {"Accept": "application/json"},
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "job_not_found", "detail": r.json()["message"]})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})

    execution = _find(cfg, r.json()["project"], token)
    if execution is not None:
        return ("resolved", execution)

    r = _check(
        requests.post(
            f"{cfg.base_url}/api/59/job/{job_id}/run",
            headers=_auth(cfg) | {"Content-Type": "application/json"},
            json=_stamp(args, token, ("loglevel", "asUser", "filter", "runAtTime")),
            timeout=10,
        )
    )
    if r.status_code == 409:
        # api.error.execution.conflict: the job's own limit on concurrent
        # executions. It clears when the execution now running ends.
        raise Exception("release", r.text)
    if r.status_code == 404:
        return ("rejected", {"code": "job_not_found", "detail": r.json()["message"]})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    return ("resolved", r.json())
```

### 4.3 job.retry

| | |
|---|---|
| **Documentation** | [API Reference — Retry a Job based on execution](https://raw.githubusercontent.com/rundeck/docs/6.2.0/docs/api/index.md) |

```json
{ "func": "job.retry", "args": { ... } }
```

### 4.3.1 Promise Param Schema

```json
{
  "description": "Re-run a job over a prior execution — on that execution's failed nodes, or on the same node set — and observe the new execution to a terminal state. Resolves when it reaches status \"succeeded\"; rejects execution_failed, execution_failed_with_retry, execution_aborted, execution_timedout or execution_other on the other terminal statuses, execution_not_found if the prior execution is unknown, job_not_found if the job UUID is unknown, execution_not_retryable if the prior execution cannot be retried through this job, invalid_request if Rundeck refuses the request, and execution_deleted if the new execution is removed before it finishes. Rundeck retries an execution only through the job that ran it, and only while a failed-node list exists for it: a succeeded execution, and a failed one that never dispatched to a node, have none and are refused with 404 api.error.item.doesnotexist \"Failed node List for execution ID does not exist\" — failedNodes false does not lift that precondition, it only widens the node set of a retry Rundeck already allows. Duration is the job's own runtime — seconds to hours. Anything left unset is inherited from the prior execution: the options given here are merged over that execution's, not substituted for them. The argString form job.run accepts is not offered here — this endpoint ignores argString in a JSON body, so option values must be given as options.",
  "type": "object",
  "properties": {
    "id": {
      "type": "string",
      "description": "Job UUID. Enumerate via job.list."
    },
    "executionId": {
      "type": ["string", "integer"],
      "description": "Execution to retry. Enumerate via execution.list."
    },
    "failedNodes": {
      "type": "boolean",
      "description": "true runs only the prior execution's failed nodes; false runs the same node set it targeted. true when omitted."
    },
    "options": {
      "type": "object",
      "description": "Option name to value, merged over the prior execution's options. The job's declared options are in job.get.",
      "additionalProperties": true
    },
    "loglevel": {
      "type": "string",
      "enum": ["DEBUG", "VERBOSE", "INFO", "WARN", "ERROR"]
    },
    "asUser": {
      "type": "string",
      "description": "Username to record as the one who ran the job. Requires runAs authorization."
    }
  },
  "required": ["id", "executionId"]
}
```

### 4.3.2 Promise Value Schema

#### Resolved

Same as 4.1.2.

#### Rejected

```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "enum": [
        "job_not_found",
        "execution_not_found",
        "execution_not_retryable",
        "invalid_request",
        "execution_deleted",
        "execution_failed",
        "execution_failed_with_retry",
        "execution_aborted",
        "execution_timedout",
        "execution_other"
      ]
    },
    "detail": {
      "description": "job_not_found: = response.body.message of the retry 404, which names the execution rather than the job because the endpoint resolves the execution within the job; execution_not_found: = response.body.message of the prior execution's 404, or of the retry 404 when the execution was deleted after it was read; execution_not_retryable: = response.body.message of the retry 404; invalid_request: the error body; execution_failed / execution_failed_with_retry / execution_aborted / execution_timedout / execution_other: = response.body (the terminal execution record); execution_deleted: absent"
    }
  },
  "required": ["code"]
}
```

### 4.3.3 Integration Request

```
POST /api/59/job/{promise.param.id}/retry/{promise.param.executionId} → 200
Content-Type: application/json
```

```json
{
  "type": "object",
  "properties": {
    "failedNodes": {
      "type": "boolean",
      "description": "= promise.param.failedNodes"
    },
    "options": {
      "type": "object",
      "description": "= promise.param.options, plus rescorr = sanitize(promise.id)",
      "additionalProperties": true
    },
    "loglevel": {
      "type": "string",
      "description": "= promise.param.loglevel"
    },
    "asUser": {
      "type": "string",
      "description": "= promise.param.asUser"
    }
  },
  "required": ["options"]
}
```

### 4.3.4 Integration Response

Same as 4.1.4.

### 4.3.5 Implementation

| | |
|---|---|
| **Algebra** | `call + poll` |
| **Invocation** | `fetch_then_create` |
| **Monitoring** | `request_poll` |

```python
def job_retry(cfg, promise):
    args = promise.param["args"]
    job_id = quote(args["id"], safe="")
    prior_id = quote(str(args["executionId"]), safe="")
    token = sanitize(promise.id)

    # The prior execution names the project the stamp is searchable in.
    r = _check(
        requests.get(
            f"{cfg.base_url}/api/59/execution/{prior_id}",
            headers=_auth(cfg),
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "execution_not_found", "detail": r.json()["message"]})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    prior = r.json()

    # This endpoint ignores argString in a JSON body; options is the form it
    # honours, and it merges with the prior execution's options rather than
    # replacing them, an explicit value winning — so the stamp is always this
    # promise's, never one inherited from the execution being retried.
    body = {k: args[k] for k in ("failedNodes", "loglevel", "asUser") if k in args}
    body["options"] = dict(args.get("options") or {}, **{CORR: token})

    execution = _find(cfg, prior["project"], token)
    if execution is None:
        r = _check(
            requests.post(
                f"{cfg.base_url}/api/59/job/{job_id}/retry/{prior_id}",
                headers=_auth(cfg) | {"Content-Type": "application/json"},
                json=body,
                timeout=10,
            )
        )
        if r.status_code == 409:
            # api.error.execution.conflict: the job's own limit on concurrent
            # executions. It clears when the execution now running ends.
            raise Exception("release", r.text)
        if r.status_code == 404:
            # One status, three causes: an unknown job, an execution this job
            # cannot resolve ("Execution ID does not exist", which is also what
            # an unknown job id answers), and an execution with no failed node
            # list ("Failed node List for execution ID does not exist"). The
            # message distinguishes them only by wording, so both ids are
            # re-read instead.
            detail = r.json()["message"]
            j = _check(
                requests.get(
                    f"{cfg.base_url}/api/59/job/{job_id}/info",
                    headers=_auth(cfg) | {"Accept": "application/json"},
                    timeout=10,
                )
            )
            if j.status_code == 404:
                return ("rejected", {"code": "job_not_found", "detail": detail})
            e = _check(
                requests.get(
                    f"{cfg.base_url}/api/59/execution/{prior_id}",
                    headers=_auth(cfg),
                    timeout=10,
                )
            )
            if e.status_code == 404:
                return ("rejected", {"code": "execution_not_found", "detail": detail})
            return ("rejected", {"code": "execution_not_retryable", "detail": detail})
        if r.status_code >= 400:
            return ("rejected", {"code": "invalid_request", "detail": r.text})
        execution = r.json()

    execution = _await(cfg, execution["id"], cfg.poll_job_retry.total_seconds(), promise)
    if execution is None:
        return ("rejected", {"code": "execution_deleted"})
    return _settle(execution)
```

### 4.4 execution.get

| | |
|---|---|
| **Documentation** | [API Reference — Execution Info](https://raw.githubusercontent.com/rundeck/docs/6.2.0/docs/api/index.md) |

```json
{ "func": "execution.get", "args": { ... } }
```

### 4.4.1 Promise Param Schema

```json
{
  "description": "Read one execution record — its status, timings, target nodes and the job it came from. Rejects not_found if the execution does not exist. A plain read — not the completion mechanism; job.run observes independently.",
  "type": "object",
  "properties": {
    "id": {
      "type": ["string", "integer"],
      "description": "Execution id. Enumerate via execution.list."
    }
  },
  "required": ["id"]
}
```

### 4.4.2 Promise Value Schema

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
      "description": "not_found: = response.body.message of the 404; invalid_request: the error body"
    }
  },
  "required": ["code"]
}
```

### 4.4.3 Integration Request

```
GET /api/59/execution/{promise.param.id} → 200
```

### 4.4.4 Integration Response

Same as 4.1.4.

### 4.4.5 Implementation

| | |
|---|---|
| **Algebra** | `call` |
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def execution_get(cfg, promise):
    args = promise.param["args"]
    exec_id = quote(str(args["id"]), safe="")

    r = _check(
        requests.get(
            f"{cfg.base_url}/api/59/execution/{exec_id}",
            headers=_auth(cfg),
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.json()["message"]})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    return ("resolved", r.json())
```

### 4.5 execution.list

| | |
|---|---|
| **Documentation** | [API Reference — Execution Query](https://raw.githubusercontent.com/rundeck/docs/6.2.0/docs/api/index.md) |

```json
{ "func": "execution.list", "args": { ... } }
```

### 4.5.1 Promise Param Schema

```json
{
  "description": "Query a project's executions, filtered and paged. Rejects not_found if the project does not exist. Scope the query to one job with jobIdListFilter: the job-scoped variant GET /api/59/job/[ID]/executions is the same query narrowed, and its status, max and offset filters are all reachable here; its includeJobRef, which folds in the executions of jobs referenced by that job, is the one filter it has that this query does not. Running executions are statusFilter \"running\". Every project at once is project \"*\", which only GET /api/59/project/*/executions/running answers — the query endpoint rejects \"*\" with 404 api.error.project.missing — so that request returns the running executions of every project and takes jobIdFilter, includePostponed, max and offset; every other filter is dropped for it. A plain read — not the completion mechanism; job.run observes independently.",
  "type": "object",
  "properties": {
    "project": {
      "type": "string",
      "description": "Project name. Enumerate via project.list."
    },
    "statusFilter": {
      "type": "string",
      "enum": ["running", "succeeded", "failed", "aborted"],
      "description": "Rundeck accepts only these four; any other status string matches nothing rather than erroring."
    },
    "jobIdListFilter": {
      "type": "array",
      "description": "Job UUIDs to include.",
      "items": { "type": "string" }
    },
    "jobIdFilter": {
      "type": "string",
      "description": "Job UUID narrowing the cross-project running list; applies only when project is \"*\". The project-scoped query narrows by job with jobIdListFilter instead."
    },
    "includePostponed": {
      "type": "boolean",
      "description": "true also returns scheduled and queued executions in the cross-project running list; applies only when project is \"*\". The project-scoped query selects them with statusFilter instead."
    },
    "excludeJobIdListFilter": {
      "type": "array",
      "description": "Job UUIDs to exclude.",
      "items": { "type": "string" }
    },
    "jobListFilter": {
      "type": "array",
      "description": "Full job paths to include, as \"group/name\", or \"name\" when the job has no group.",
      "items": { "type": "string" }
    },
    "excludeJobListFilter": {
      "type": "array",
      "description": "Full job paths to exclude.",
      "items": { "type": "string" }
    },
    "groupPath": {
      "type": "string",
      "description": "Group or partial group path to include. \"-\" matches top-level jobs only."
    },
    "groupPathExact": {
      "type": "string",
      "description": "Exact group path to match. \"-\" matches top-level jobs only."
    },
    "excludeGroupPath": {
      "type": "string",
      "description": "Group or partial group path to exclude."
    },
    "excludeGroupPathExact": {
      "type": "string",
      "description": "Exact group path to exclude."
    },
    "jobFilter": {
      "type": "string",
      "description": "Substring of the job name to include."
    },
    "excludeJobFilter": {
      "type": "string",
      "description": "Substring of the job name to exclude."
    },
    "jobExactFilter": {
      "type": "string",
      "description": "Exact job name to match."
    },
    "excludeJobExactFilter": {
      "type": "string",
      "description": "Exact job name to exclude."
    },
    "userFilter": {
      "type": "string",
      "description": "Username that started the execution."
    },
    "abortedbyFilter": {
      "type": "string",
      "description": "Username that aborted the execution."
    },
    "executionTypeFilter": {
      "type": "string",
      "enum": ["scheduled", "user", "user-scheduled"]
    },
    "optionFilter": {
      "type": "string",
      "description": "Partial match over the execution's option values, in \"-name value\" form, e.g. \"-test 123\". Executions begun by job.run, job.submit or job.retry carry a rescorr option the plugin injected."
    },
    "recentFilter": {
      "type": "string",
      "description": "Completed within a relative period, as \"XY\": X an integer, Y one of s, n (minute), h, d, w, m, y. \"2w\" is the last two weeks."
    },
    "olderFilter": {
      "type": "string",
      "description": "Completed before a relative period, same format as recentFilter."
    },
    "begin": {
      "type": ["string", "integer"],
      "description": "Earliest completion time: a unix millisecond timestamp, or a W3C dateTime yyyy-MM-ddTHH:mm:ssZ."
    },
    "end": {
      "type": ["string", "integer"],
      "description": "Latest completion time, same format as begin."
    },
    "adhoc": {
      "type": "boolean",
      "description": "true returns only adhoc executions, false only job executions. Both when omitted, unless a job filter is set."
    },
    "max": {
      "type": "integer",
      "minimum": 0,
      "description": "Page size; 20 when omitted."
    },
    "offset": {
      "type": "integer",
      "minimum": 0,
      "description": "0 when omitted."
    }
  },
  "required": ["project"]
}
```

### 4.5.2 Promise Value Schema

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
      "description": "not_found: = response.body.message of the 404; invalid_request: the error body"
    }
  },
  "required": ["code"]
}
```

### 4.5.3 Integration Request

```
GET /api/59/project/{promise.param.project}/executions{?statusFilter,jobIdListFilter,excludeJobIdListFilter,jobListFilter,excludeJobListFilter,groupPath,groupPathExact,excludeGroupPath,excludeGroupPathExact,jobFilter,excludeJobFilter,jobExactFilter,excludeJobExactFilter,userFilter,abortedbyFilter,executionTypeFilter,optionFilter,recentFilter,olderFilter,begin,end,adhoc,max,offset = promise.param.*} → 200
GET /api/59/project/*/executions/running{?jobIdFilter,includePostponed,max,offset = promise.param.*} → 200
```

### 4.5.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "paging": {
      "type": "object",
      "properties": {
        "count": { "type": "integer", "description": "Executions in this page." },
        "total": { "type": "integer", "description": "Executions matching the query." },
        "offset": { "type": "integer" },
        "max": { "type": "integer" }
      }
    },
    "executions": {
      "type": "array",
      "items": { "description": "Same as 4.1.4" }
    }
  },
  "required": ["paging", "executions"]
}
```

### 4.5.5 Implementation

| | |
|---|---|
| **Algebra** | `call` |
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def execution_list(cfg, promise):
    args = promise.param["args"]
    project = quote(args["project"], safe="")

    # "*" is a project name only on the running list; the query endpoint
    # answers 404 api.error.project.missing for it. Both answer the same
    # paging/executions body.
    if args["project"] == "*":
        url = f"{cfg.base_url}/api/59/project/*/executions/running"
        keys = ("jobIdFilter", "includePostponed", "max", "offset")
    else:
        url = f"{cfg.base_url}/api/59/project/{project}/executions"
        keys = (
            "statusFilter",
            "jobIdListFilter",
            "excludeJobIdListFilter",
            "jobListFilter",
            "excludeJobListFilter",
            "groupPath",
            "groupPathExact",
            "excludeGroupPath",
            "excludeGroupPathExact",
            "jobFilter",
            "excludeJobFilter",
            "jobExactFilter",
            "excludeJobExactFilter",
            "userFilter",
            "abortedbyFilter",
            "executionTypeFilter",
            "optionFilter",
            "recentFilter",
            "olderFilter",
            "begin",
            "end",
            "adhoc",
            "max",
            "offset",
        )

    # Pagination is the caller's loop (one promise per page), not ours.
    r = _check(
        requests.get(
            url,
            headers=_auth(cfg),
            params=_query(args, keys),
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.json()["message"]})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    return ("resolved", r.json())
```

### 4.6 execution.output

| | |
|---|---|
| **Documentation** | [API Reference — Execution Output](https://raw.githubusercontent.com/rundeck/docs/6.2.0/docs/api/index.md) |

```json
{ "func": "execution.output", "args": { ... } }
```

### 4.6.1 Promise Param Schema

```json
{
  "description": "Read an execution's log, whole or in a window, on a running or a finished execution. The response carries the read position (offset, lastModified) a caller advances to continue, and execCompleted tells them when there is no more. Narrow to one node with nodename, to one workflow step with stepctx, or to both. Rejects not_found if the execution does not exist. A plain read — not the completion mechanism; job.run observes independently.",
  "type": "object",
  "properties": {
    "id": {
      "type": ["string", "integer"],
      "description": "Execution id. Enumerate via execution.list."
    },
    "nodename": {
      "type": "string",
      "description": "Return only entries from this node."
    },
    "stepctx": {
      "type": "string",
      "description": "Return only entries from this step context, e.g. \"1\" or \"1/2/3\"."
    },
    "offset": {
      "type": ["string", "integer"],
      "description": "Byte offset into the log file to read from; 0 is the beginning. An opaque position, not a count of log text."
    },
    "lastlines": {
      "type": "integer",
      "description": "Return this many lines from the end of the available output; overrides offset."
    },
    "maxlines": {
      "type": "integer",
      "description": "Maximum entries to return forward from offset."
    },
    "lastmod": {
      "type": ["string", "integer"],
      "description": "Millisecond epoch timestamp; return entries only if the log changed since then or more data is available at offset."
    },
    "compacted": {
      "type": "boolean",
      "description": "true returns each entry with only the fields that changed from the previous one. Since API v59 every compacted entry is an object; v58 and below could return a bare string or an empty object."
    }
  },
  "required": ["id"]
}
```

### 4.6.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "description": "= response.body"
}
```

#### Rejected

Same as 4.4.2.

### 4.6.3 Integration Request

```
GET /api/59/execution/{promise.param.id}/output{?nodename,stepctx,offset,lastlines,maxlines,lastmod,compacted = promise.param.*} → 200
Accept: application/json
```

### 4.6.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "id": { "type": "string", "description": "Execution id, as a string." },
    "message": { "type": "string", "description": "Why no entries were returned." },
    "error": { "type": "string", "description": "Error case description." },
    "unmodified": { "type": "boolean", "description": "true when lastmod was sent and the log had not changed." },
    "empty": { "type": "boolean", "description": "true when the log file does not exist or is empty." },
    "offset": { "type": "string", "description": "Byte offset to read from for the next window." },
    "completed": { "type": "boolean", "description": "true when this response holds all the data the request asked for." },
    "execCompleted": { "type": "boolean", "description": "true when the execution itself has finished." },
    "hasFailedNodes": { "type": "boolean" },
    "execState": {
      "type": "string",
      "enum": ["running", "succeeded", "failed", "aborted"],
      "description": "Execution state as the log endpoint reports it — a narrower set than the execution record's status."
    },
    "lastModified": { "type": "string", "description": "Millisecond timestamp of the log file's last modification." },
    "execDuration": { "type": "integer", "description": "Milliseconds." },
    "percentLoaded": { "type": "number" },
    "totalSize": { "type": "integer", "description": "Total bytes available in the log file." },
    "compacted": { "type": "boolean" },
    "compactedAttr": { "type": "string", "description": "Entry key carrying a fully compacted entry." },
    "clusterExec": { "type": "boolean" },
    "retryBackoff": { "type": "integer" },
    "serverNodeUUID": { "type": "string", "description": "UUID of the Rundeck server holding the log — the serverUUID of GET /api/59/system/info." },
    "filter": {
      "type": "object",
      "properties": {
        "nodename": { "type": "string" },
        "stepctx": { "type": "string" }
      }
    },
    "entries": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "time": { "type": "string", "description": "HH:MM:SS." },
          "absolute_time": { "type": "string", "description": "yyyy-MM-ddTHH:mm:ssZ." },
          "level": {
            "type": "string",
            "enum": ["ERROR", "WARN", "NORMAL", "VERBOSE", "DEBUG", "OTHER"]
          },
          "log": { "type": "string" },
          "user": { "type": "string" },
          "command": { "type": "string" },
          "node": { "type": ["string", "null"] },
          "stepctx": { "type": ["string", "null"] },
          "metadata": { "type": "object" }
        }
      }
    }
  },
  "required": ["id", "offset", "completed", "execCompleted", "execState", "entries"]
}
```

### 4.6.5 Implementation

| | |
|---|---|
| **Algebra** | `call` |
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def execution_output(cfg, promise):
    args = promise.param["args"]
    exec_id = quote(str(args["id"]), safe="")

    # Tailing is the caller's loop: each promise reads one window and hands
    # back the offset and lastModified the next one starts from.
    r = _check(
        requests.get(
            f"{cfg.base_url}/api/59/execution/{exec_id}/output",
            headers=_auth(cfg) | {"Accept": "application/json"},
            params=_query(
                args,
                ("nodename", "stepctx", "offset", "lastlines", "maxlines", "lastmod", "compacted"),
            ),
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.json()["message"]})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    return ("resolved", r.json())
```

### 4.7 execution.state

| | |
|---|---|
| **Documentation** | [API Reference — Execution State](https://raw.githubusercontent.com/rundeck/docs/6.2.0/docs/api/index.md) |

```json
{ "func": "execution.state", "args": { ... } }
```

### 4.7.1 Promise Param Schema

```json
{
  "description": "Read an execution's workflow state: the overall state, the state of every step, and the state of every node within each step — the structured answer to which step failed on which node. Works on a running or a finished execution. Rejects not_found if the execution does not exist. A plain read — not the completion mechanism; job.run observes independently.",
  "type": "object",
  "properties": {
    "id": {
      "type": ["string", "integer"],
      "description": "Execution id. Enumerate via execution.list."
    }
  },
  "required": ["id"]
}
```

### 4.7.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "description": "= response.body"
}
```

#### Rejected

Same as 4.4.2.

### 4.7.3 Integration Request

```
GET /api/59/execution/{promise.param.id}/state → 200
Accept: application/json
```

### 4.7.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "executionId": { "type": "integer" },
    "serverNode": { "type": "string", "description": "Name of the server node." },
    "completed": { "type": "boolean" },
    "executionState": {
      "type": "string",
      "enum": ["WAITING", "RUNNING", "RUNNING_HANDLER", "SUCCEEDED", "FAILED", "ABORTED", "NODE_PARTIAL_SUCCEEDED", "NODE_MIXED", "NOT_STARTED"],
      "description": "RUNNING_HANDLER, NODE_PARTIAL_SUCCEEDED, NODE_MIXED and NOT_STARTED apply to steps and nodes only, never to the overall execution."
    },
    "startTime": { "type": "string", "description": "yyyy-MM-ddTHH:mm:ssZ." },
    "endTime": { "type": ["string", "null"], "description": "yyyy-MM-ddTHH:mm:ssZ; null until complete." },
    "updateTime": { "type": "string", "description": "yyyy-MM-ddTHH:mm:ssZ." },
    "stepCount": { "type": "integer" },
    "allNodes": {
      "type": "array",
      "description": "Every node targeted in some workflow.",
      "items": { "type": "string" }
    },
    "targetNodes": {
      "type": "array",
      "description": "Target nodes of this workflow.",
      "items": { "type": "string" }
    },
    "nodes": {
      "type": "object",
      "description": "Node name to the list of steps that node executed.",
      "additionalProperties": {
        "type": "array",
        "items": {
          "type": "object",
          "properties": {
            "stepctx": { "type": "string", "description": "Step context identifier, e.g. \"1\" or \"2/1\"." },
            "executionState": { "type": "string" }
          }
        }
      }
    },
    "steps": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "id": { "type": "string", "description": "Step number within its workflow, from 1." },
          "stepctx": { "type": "string" },
          "executionState": { "type": "string" },
          "startTime": { "type": "string" },
          "endTime": { "type": ["string", "null"] },
          "updateTime": { "type": "string" },
          "duration": { "type": "integer", "description": "Milliseconds." },
          "nodeStep": {
            "type": "boolean",
            "description": "true when the step targets each node directly, in which case nodeStates is present."
          },
          "nodeStates": {
            "type": "object",
            "description": "Node name to that node's state for this step.",
            "additionalProperties": {
              "type": "object",
              "properties": {
                "executionState": { "type": "string" },
                "startTime": { "type": "string" },
                "endTime": { "type": ["string", "null"] },
                "updateTime": { "type": "string" },
                "duration": { "type": "integer" }
              }
            }
          },
          "hasSubworkflow": { "type": "boolean" },
          "workflow": {
            "type": "object",
            "description": "A nested workflow section with the same stepCount, targetNodes and steps structure."
          },
          "parameterStates": { "type": "object" }
        }
      }
    }
  },
  "required": ["executionId", "executionState", "completed", "stepCount", "steps"]
}
```

### 4.7.5 Implementation

| | |
|---|---|
| **Algebra** | `call` |
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def execution_state(cfg, promise):
    args = promise.param["args"]
    exec_id = quote(str(args["id"]), safe="")

    r = _check(
        requests.get(
            f"{cfg.base_url}/api/59/execution/{exec_id}/state",
            headers=_auth(cfg) | {"Accept": "application/json"},
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.json()["message"]})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    return ("resolved", r.json())
```

### 4.8 execution.abort

| | |
|---|---|
| **Documentation** | [API Reference — Aborting Executions](https://raw.githubusercontent.com/rundeck/docs/6.2.0/docs/api/index.md) |

```json
{ "func": "execution.abort", "args": { ... } }
```

### 4.8.1 Promise Param Schema

```json
{
  "description": "Abort a running execution and wait until it has actually stopped. Resolves with the execution record once it reaches a terminal status — usually \"aborted\", but an execution that finishes while the abort is in flight settles \"succeeded\", \"failed\", \"timedout\" or \"other\" instead, and abort does not judge which. Rejects abort_failed if Rundeck refuses the abort while the execution is still going, not_found if the execution does not exist, invalid_request if Rundeck refuses the request, and deleted if the execution is removed before it stops. Duration is however long the execution takes to wind down — seconds.",
  "type": "object",
  "properties": {
    "id": {
      "type": ["string", "integer"],
      "description": "Execution id. Enumerate via execution.list."
    },
    "asUser": {
      "type": "string",
      "description": "Username to record as the one who aborted the execution. Requires runAs authorization."
    },
    "forceIncomplete": {
      "type": "boolean",
      "description": "true marks a running execution \"incomplete\" rather than waiting for it to yield."
    }
  },
  "required": ["id"]
}
```

### 4.8.2 Promise Value Schema

#### Resolved

Same as 4.1.2.

#### Rejected

```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "enum": ["not_found", "abort_failed", "invalid_request", "deleted"]
    },
    "detail": {
      "description": "not_found: = response.body.message of the 404; abort_failed: = response.body.abort.reason; invalid_request: the error body; deleted: absent"
    }
  },
  "required": ["code"]
}
```

### 4.8.3 Integration Request

```
POST /api/59/execution/{promise.param.id}/abort{?asUser,forceIncomplete = promise.param.*} → 200
```

### 4.8.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "abort": {
      "type": "object",
      "properties": {
        "status": {
          "type": "string",
          "enum": ["pending", "failed", "aborted"],
          "description": "pending means the abort was accepted and the execution has not stopped yet. failed on an execution that already stopped carries reason \"Job is not running\"."
        },
        "reason": {
          "type": "string",
          "description": "Why the abort ended in this status. Accompanies failed (\"Job is not running\"), and aborted when the execution was marked incomplete (\"Marked as incomplete\"). Absent from pending."
        }
      }
    },
    "execution": {
      "type": "object",
      "properties": {
        "id": { "type": "string", "description": "Execution id, as a string." },
        "status": { "type": "string", "description": "The execution's status at the moment of the abort request." },
        "href": { "type": "string" },
        "permalink": { "type": "string" }
      }
    }
  },
  "required": ["abort", "execution"]
}
```

### 4.8.5 Implementation

| | |
|---|---|
| **Algebra** | `call + poll` |
| **Invocation** | `create` |
| **Monitoring** | `request_poll` |

```python
def execution_abort(cfg, promise):
    args = promise.param["args"]
    exec_id = quote(str(args["id"]), safe="")

    # The OpenAPI defines only POST on this path (operationId
    # apiExecutionAbort); the API Reference narrative writes it GET. Both
    # methods answer, and the OpenAPI is the authority.
    #
    # A repeated abort is free: on an execution that has already stopped
    # Rundeck answers 200 with abort.status "failed", reason "Job is not
    # running", and the execution's terminal status.
    r = _check(
        requests.post(
            f"{cfg.base_url}/api/59/execution/{exec_id}/abort",
            headers=_auth(cfg),
            params=_query(args, ("asUser", "forceIncomplete")),
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.json()["message"]})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    body = r.json()
    if body["abort"]["status"] == "failed" and body["execution"]["status"] not in TERMINAL:
        return ("rejected", {"code": "abort_failed", "detail": body["abort"].get("reason")})

    execution = _await(cfg, args["id"], cfg.poll_execution_abort.total_seconds(), promise)
    if execution is None:
        return ("rejected", {"code": "deleted"})
    return ("resolved", {k: execution.get(k) for k in RESULT})
```

### 4.9 execution.delete

| | |
|---|---|
| **Documentation** | [API Reference — Delete an Execution](https://raw.githubusercontent.com/rundeck/docs/6.2.0/docs/api/index.md) |

```json
{ "func": "execution.delete", "args": { ... } }
```

### 4.9.1 Promise Param Schema

```json
{
  "description": "Delete one execution record and its stored log. Requires the delete_execution action for the project in the application context. Resolves empty on 204. Rejects not_found if the execution does not exist — which is also the answer after a first delivery already deleted it, since Rundeck offers no key that would tell the two apart. An execution that is still running or scheduled cannot be deleted: Rundeck answers 500 and the attempt is retried until it finishes. The batch forms POST /api/59/executions/delete and DELETE /api/59/job/[ID]/executions are not exposed; delete one promise per execution.",
  "type": "object",
  "properties": {
    "id": {
      "type": ["string", "integer"],
      "description": "Execution id. Enumerate via execution.list."
    }
  },
  "required": ["id"]
}
```

### 4.9.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "description": "= response.body (204 No Content: empty)",
  "properties": {}
}
```

#### Rejected

Same as 4.4.2.

### 4.9.3 Integration Request

```
DELETE /api/59/execution/{promise.param.id} → 204
```

### 4.9.4 Integration Response

```json
{
  "type": "object",
  "description": "204 No Content: no body",
  "properties": {}
}
```

### 4.9.5 Implementation

| | |
|---|---|
| **Algebra** | `call` |
| **Invocation** | `create` |
| **Monitoring** | `request_response` |

```python
def execution_delete(cfg, promise):
    args = promise.param["args"]
    exec_id = quote(str(args["id"]), safe="")

    # Re-delivery after a delete that landed sees the same 404 a wrong id
    # sees; Rundeck keeps no tombstone that would separate them.
    r = _check(
        requests.delete(
            f"{cfg.base_url}/api/59/execution/{exec_id}",
            headers=_auth(cfg),
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.json()["message"]})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    return ("resolved", {})
```

### 4.10 job.get

| | |
|---|---|
| **Documentation** | [API Reference — Getting a Job Definition](https://raw.githubusercontent.com/rundeck/docs/6.2.0/docs/api/index.md) |

```json
{ "func": "job.get", "args": { ... } }
```

### 4.10.1 Promise Param Schema

```json
{
  "description": "Read one job's definition — its workflow steps, node filters, schedule and the options it declares, which is what constrains the options and argString of job.run. Returned in the job-json-v44 format; the XML and YAML export formats are not exposed. Rejects not_found if the job UUID is unknown. Creating, updating and deleting job definitions is not exposed: Rundeck treats a job definition as schema, maintained through the UI or SCM rather than by a workflow.",
  "type": "object",
  "properties": {
    "id": {
      "type": "string",
      "description": "Job UUID. Enumerate via job.list."
    }
  },
  "required": ["id"]
}
```

### 4.10.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "description": "= response.body[0] — the single job definition Rundeck wraps in a one-element array"
}
```

#### Rejected

Same as 4.4.2.

### 4.10.3 Integration Request

```
GET /api/59/job/{promise.param.id}?format=json → 200
Accept: application/json
```

### 4.10.4 Integration Response

```json
{
  "type": "array",
  "description": "job-json-v44: one element, the requested job.",
  "items": {
    "type": "object",
    "properties": {
      "id": { "type": "string", "description": "Job UUID." },
      "uuid": { "type": "string", "description": "Job UUID." },
      "name": { "type": "string" },
      "group": { "type": "string" },
      "description": { "type": "string" },
      "loglevel": {
        "type": "string",
        "enum": ["DEBUG", "VERBOSE", "INFO", "WARN", "ERROR"]
      },
      "executionEnabled": { "type": "boolean" },
      "scheduleEnabled": { "type": "boolean" },
      "multipleExecutions": { "type": "boolean" },
      "nodeFilterEditable": { "type": "boolean" },
      "user": { "type": "string" },
      "options": {
        "type": "array",
        "description": "The options this job declares; their names are the keys job.run accepts.",
        "items": {
          "type": "object",
          "properties": {
            "name": { "type": "string" },
            "value": { "type": "string", "description": "Default value." },
            "required": { "type": "boolean" },
            "description": { "type": "string" },
            "values": { "type": "array", "items": { "type": "string" } },
            "enforced": { "type": "boolean" },
            "type": { "type": "string" }
          }
        }
      },
      "sequence": {
        "type": "object",
        "properties": {
          "keepgoing": { "type": "boolean" },
          "strategy": { "type": "string" },
          "commands": { "type": "array", "items": { "type": "object" } }
        }
      },
      "nodefilters": { "type": "object" },
      "schedule": { "type": "object" },
      "plugins": { "type": "object" }
    },
    "required": ["name", "uuid", "sequence"]
  }
}
```

### 4.10.5 Implementation

| | |
|---|---|
| **Algebra** | `call` |
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def job_get(cfg, promise):
    args = promise.param["args"]
    job_id = quote(args["id"], safe="")

    r = _check(
        requests.get(
            f"{cfg.base_url}/api/59/job/{job_id}",
            headers=_auth(cfg) | {"Accept": "application/json"},
            params={"format": "json"},
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.json()["message"]})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    return ("resolved", r.json()[0])
```

### 4.11 job.list

| | |
|---|---|
| **Documentation** | [API Reference — Listing Jobs](https://raw.githubusercontent.com/rundeck/docs/6.2.0/docs/api/index.md) |

```json
{ "func": "job.list", "args": { ... } }
```

### 4.11.1 Promise Param Schema

```json
{
  "description": "List a project's jobs — the read that turns a job name into the UUID job.run and job.get need. Rejects not_found if the project does not exist. Enabling and disabling a job's executions or schedule is not exposed: like the job definition itself, Rundeck expects that to be administered out of band.",
  "type": "object",
  "properties": {
    "project": {
      "type": "string",
      "description": "Project name. Enumerate via project.list."
    },
    "idlist": {
      "type": "string",
      "description": "Comma-separated job UUIDs to include."
    },
    "groupPath": {
      "type": "string",
      "description": "Group or partial group path to include; \"*\" is every group and is the default, \"-\" matches top-level jobs only. Cannot be combined with groupPathExact."
    },
    "groupPathExact": {
      "type": "string",
      "description": "Exact group path to match; \"-\" matches top-level jobs only."
    },
    "jobFilter": {
      "type": "string",
      "description": "Substring of the job name."
    },
    "jobExactFilter": {
      "type": "string",
      "description": "Exact job name."
    },
    "scheduledFilter": {
      "type": "boolean",
      "description": "true returns only scheduled jobs, false only unscheduled ones."
    },
    "serverNodeUUIDFilter": {
      "type": "string",
      "description": "In cluster mode, the server UUID whose scheduled jobs to return."
    },
    "tags": {
      "type": "string",
      "description": "Comma-separated tags; returns jobs carrying any of them."
    },
    "max": {
      "type": "integer",
      "minimum": 0,
      "description": "Page size."
    },
    "offset": {
      "type": "integer",
      "minimum": 0,
      "description": "Use with max to page."
    }
  },
  "required": ["project"]
}
```

### 4.11.2 Promise Value Schema

#### Resolved

```json
{
  "type": "array",
  "description": "= response.body"
}
```

#### Rejected

Same as 4.4.2.

### 4.11.3 Integration Request

```
GET /api/59/project/{promise.param.project}/jobs{?idlist,groupPath,groupPathExact,jobFilter,jobExactFilter,scheduledFilter,serverNodeUUIDFilter,tags,max,offset = promise.param.*} → 200
```

### 4.11.4 Integration Response

```json
{
  "type": "array",
  "items": {
    "type": "object",
    "properties": {
      "id": { "type": "string", "description": "Job UUID — the id job.run takes." },
      "name": { "type": "string" },
      "group": { "type": ["string", "null"] },
      "project": { "type": "string" },
      "description": { "type": "string" },
      "href": { "type": "string" },
      "permalink": { "type": "string" },
      "scheduled": { "type": "boolean", "description": "The job has a schedule." },
      "scheduleEnabled": { "type": "boolean" },
      "enabled": { "type": "boolean", "description": "Executions are enabled for the job." },
      "serverNodeUUID": { "type": "string", "description": "Cluster schedule owner." },
      "serverOwner": { "type": "boolean" },
      "createdBy": { "type": ["string", "null"] },
      "lastModifiedBy": { "type": ["string", "null"] },
      "created": { "type": ["string", "null"], "description": "ISO-8601." },
      "lastModified": { "type": ["string", "null"], "description": "ISO-8601." }
    },
    "required": ["id", "name", "project"]
  }
}
```

### 4.11.5 Implementation

| | |
|---|---|
| **Algebra** | `call` |
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def job_list(cfg, promise):
    args = promise.param["args"]
    project = quote(args["project"], safe="")

    params = _query(
        args,
        (
            "idlist",
            "groupPath",
            "groupPathExact",
            "jobFilter",
            "jobExactFilter",
            "scheduledFilter",
            "serverNodeUUIDFilter",
            "tags",
            "max",
            "offset",
        ),
    )
    r = _check(
        requests.get(
            f"{cfg.base_url}/api/59/project/{project}/jobs",
            headers=_auth(cfg),
            params=params,
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found", "detail": r.json()["message"]})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    return ("resolved", r.json())
```

### 4.12 project.list

| | |
|---|---|
| **Documentation** | [API Reference — Listing Projects](https://raw.githubusercontent.com/rundeck/docs/6.2.0/docs/api/index.md) |

```json
{ "func": "project.list", "args": { ... } }
```

### 4.12.1 Promise Param Schema

```json
{
  "description": "List the projects on the server — the read that names the project job.list and execution.list are scoped by. Creating, deleting, configuring and archiving projects is not exposed: that is instance administration, not a motion a workflow performs.",
  "type": "object",
  "properties": {
    "meta": {
      "type": "string",
      "description": "Comma-separated metadata component names to include, or \"*\" for all."
    }
  }
}
```

### 4.12.2 Promise Value Schema

#### Resolved

```json
{
  "type": "array",
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
      "description": "= the error body"
    }
  },
  "required": ["code"]
}
```

### 4.12.3 Integration Request

```
GET /api/59/projects{?meta = promise.param.*} → 200
```

### 4.12.4 Integration Response

```json
{
  "type": "array",
  "items": {
    "type": "object",
    "properties": {
      "name": { "type": "string", "description": "Project name — the project job.list and execution.list take." },
      "description": { "type": "string" },
      "url": { "type": "string" },
      "label": { "type": "string" },
      "created": { "type": "string", "description": "ISO-8601." },
      "meta": {
        "type": "array",
        "description": "Present when meta was requested.",
        "items": {
          "type": "object",
          "properties": {
            "name": { "type": "string", "description": "Metadata component name, e.g. \"authz\" or \"config\"." },
            "data": { "type": "object", "description": "That component's values." }
          }
        }
      }
    },
    "required": ["name", "url"]
  }
}
```

### 4.12.5 Implementation

| | |
|---|---|
| **Algebra** | `call` |
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def project_list(cfg, promise):
    args = promise.param["args"]

    r = _check(
        requests.get(
            f"{cfg.base_url}/api/59/projects",
            headers=_auth(cfg),
            params=_query(args, ("meta",)),
            timeout=10,
        )
    )
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.text})
    return ("resolved", r.json())
```

## 5. Test

### 5.1 Base Image

```dockerfile
FROM rundeck/rundeck:6.1.0
USER root
RUN apt-get update \
 && apt-get install -y --no-install-recommends python3 python3-requests \
 && rm -rf /var/lib/apt/lists/*
USER rundeck
```

### 5.2 Run

```sh
docker rm -f plugin-rundeck-test >/dev/null 2>&1 || true
docker build -t plugin-rundeck-test spec/
docker run -d --name plugin-rundeck-test -p 4440:4440 -e RUNDECK_GRAILS_URL=http://localhost:4440 plugin-rundeck-test

until [ "$(curl -s -o /dev/null -w '%{http_code}' http://localhost:4440/api/59/system/info)" = "403" ]; do sleep 5; done

JAR=$(mktemp)
until curl -s -c "$JAR" -b "$JAR" -L -o /dev/null -X POST http://localhost:4440/j_security_check -d j_username=admin -d j_password=admin \
  && RUNDECK_API_TOKEN=$(curl -s -c "$JAR" -b "$JAR" -X POST http://localhost:4440/api/59/tokens \
       -H 'Content-Type: application/json' -d '{"user":"admin","roles":"*","name":"resonate"}' \
     | python3 -c 'import json,sys; print(json.load(sys.stdin)["token"])' 2>/dev/null) \
  && [ -n "$RUNDECK_API_TOKEN" ]; do sleep 5; done

curl -s -o /dev/null -X POST -H "X-Rundeck-Auth-Token: $RUNDECK_API_TOKEN" \
  -H 'Content-Type: application/json' -d '{"name":"resonate"}' http://localhost:4440/api/59/projects

JOBS=$(mktemp)
cat > "$JOBS" <<'YAML'
- name: fixture-ok
  group: ''
  project: resonate
  description: sleeps for the requested number of seconds, then succeeds
  loglevel: INFO
  executionEnabled: true
  multipleExecutions: true
  options:
    - name: seconds
      value: '0'
  sequence:
    keepgoing: false
    strategy: node-first
    commands:
      - exec: sleep ${option.seconds}
      - exec: echo ok
- name: fixture-fail
  group: ''
  project: resonate
  description: exits non-zero
  loglevel: INFO
  executionEnabled: true
  multipleExecutions: true
  sequence:
    keepgoing: false
    strategy: node-first
    commands:
      - exec: /bin/false
YAML
IMPORTED=$(curl -s -X POST -H "X-Rundeck-Auth-Token: $RUNDECK_API_TOKEN" -H 'Content-Type: application/yaml' \
  --data-binary @"$JOBS" "http://localhost:4440/api/59/project/resonate/jobs/import?dupeOption=update")

export RUNDECK_BASE_URL=http://localhost:4440
export RUNDECK_API_TOKEN
export RUNDECK_FIXTURE_OK=$(printf '%s' "$IMPORTED" | python3 -c 'import json,sys; print([j["id"] for j in json.load(sys.stdin)["succeeded"] if j["name"] == "fixture-ok"][0])')
export RUNDECK_FIXTURE_FAIL=$(printf '%s' "$IMPORTED" | python3 -c 'import json,sys; print([j["id"] for j in json.load(sys.stdin)["succeeded"] if j["name"] == "fixture-fail"][0])')
```
