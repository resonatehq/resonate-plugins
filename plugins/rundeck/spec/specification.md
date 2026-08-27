# Rundeck

| | |
|---|---|
| **API** | `{base_url}/api/59` |
| **Idempotency** | No client-supplied run key: `POST /api/59/job/[ID]/run` accepts no execution id, and the `meta.KEY` values it stamps are not returned by any documented read. Executions are locatable by option value: every job accepts undeclared options, and `GET /api/59/project/[PROJECT]/executions?optionFilter=` matches them — `sanitize(promise.id)` is injected as the `rescorr` option (the project comes from `GET /api/59/job/[ID]/info`) |
| **Reviewed by** | Claude Opus, 2026-08-26 |

**Notes**

| | |
|---|---|
| **OpenAPI** | `https://docs.rundeck.com/docs/files/rundeck-api.yml` |
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
| `api_token` | `String` | | `…` |
| `poll` | `Duration` | `5s` | `5s` |

## 3. Authentication & Authorization

| | |
|---|---|
| **Documentation** | [API Token Authentication](https://docs.rundeck.com/docs/api/#api-token-authentication) |
| **Probe** | `GET /api/59/system/info` → `200` |

```
X-Rundeck-Auth-Token: {api_token}
```

## 4. Operations

### 4.1 job.run

| | |
|---|---|
| **Documentation** | [Running a Job](https://docs.rundeck.com/docs/api/#running-a-job) |

```json
{ "func": "job.run", "args": { ... } }
```

### 4.1.1 Promise Param Schema

```json
{
  "description": "Run a job and observe its execution to a terminal state. Resolves when the execution reaches status \"succeeded\"; rejects execution_failed on \"failed\", execution_aborted on \"aborted\", execution_timedout on \"timedout\", execution_failed_with_retry on \"failed-with-retry\" (this execution is over and Rundeck started a separate retry execution that is not observed here), execution_other on \"other\" (a custom exit status) or on any status outside the documented set, job_not_found if the job id is unknown, invalid_request on any other 4xx from the run request — e.g. an option value the job's option validation refuses — and deleted if the execution is deleted before its terminal state is observed (Rundeck only permits deleting finished executions). Duration is the job's own runtime — seconds to hours. An execution created with runAtTime stays in status \"scheduled\" until that time arrives and is polled until then, so size timeoutAt to cover the delay plus the run.",
  "type": "object",
  "properties": {
    "id": {
      "type": "string",
      "description": "Job UUID. Enumerate via job.list."
    },
    "options": {
      "type": "object",
      "description": "Job option values keyed by option name; values are strings. Valid names and their constraints are the job's declared options — see job.get. Overrides argString when both are given.",
      "additionalProperties": true
    },
    "argString": {
      "type": "string",
      "description": "Option values in command-line form: \"-opt value -opt2 value\". Ignored when options is given."
    },
    "loglevel": {
      "type": "string",
      "enum": ["DEBUG", "VERBOSE", "INFO", "WARN", "ERROR"],
      "description": "Log level for this execution; the job's own loglevel when omitted."
    },
    "asUser": {
      "type": "string",
      "description": "Username recorded as the user who ran the job. Requires the runAs permission."
    },
    "filter": {
      "type": "string",
      "description": "Node filter string restricting the nodes this execution targets."
    },
    "runAtTime": {
      "type": "string",
      "format": "date-time",
      "description": "ISO-8601 date and time with timezone and optional milliseconds, e.g. \"2016-11-23T12:20:55-0800\". The execution is created in status \"scheduled\" and starts at that time."
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
      "type": "integer",
      "description": "= response.body.id"
    },
    "href": {
      "type": "string",
      "description": "= response.body.href"
    },
    "permalink": {
      "type": "string",
      "description": "= response.body.permalink"
    },
    "status": {
      "type": "string",
      "description": "= response.body.status"
    },
    "project": {
      "type": "string",
      "description": "= response.body.project"
    },
    "user": {
      "type": "string",
      "description": "= response.body.user"
    },
    "date-started": {
      "type": "object",
      "description": "= response.body.date-started"
    },
    "date-ended": {
      "type": "object",
      "description": "= response.body.date-ended"
    },
    "job": {
      "type": "object",
      "description": "= response.body.job"
    },
    "description": {
      "type": "string",
      "description": "= response.body.description"
    },
    "argstring": {
      "type": ["string", "null"],
      "description": "= response.body.argstring"
    },
    "successfulNodes": {
      "type": "array",
      "description": "= response.body.successfulNodes",
      "items": {
        "type": "string"
      }
    },
    "failedNodes": {
      "type": "array",
      "description": "= response.body.failedNodes",
      "items": {
        "type": "string"
      }
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
      "enum": [
        "job_not_found",
        "invalid_request",
        "execution_failed",
        "execution_aborted",
        "execution_timedout",
        "execution_failed_with_retry",
        "execution_other",
        "deleted"
      ]
    },
    "detail": {
      "description": "job_not_found / invalid_request: = response.body.message of the 4xx; execution_failed / execution_aborted / execution_timedout / execution_failed_with_retry / execution_other: = response.body (the terminal execution object); deleted: absent"
    }
  },
  "required": ["code"]
}
```

### 4.1.3 Integration Request

```
POST /api/59/job/{promise.param.id}/run → 200
Content-Type: application/json
Accept: application/json
```

```json
{
  "type": "object",
  "properties": {
    "options": {
      "type": "object",
      "description": "= promise.param.options, plus \"rescorr\": = sanitize(promise.id) — the injected identity; appended to argString instead when only argString is given",
      "additionalProperties": true
    },
    "argString": {
      "type": "string",
      "description": "= promise.param.argString, plus \" -rescorr \" + sanitize(promise.id) when options is absent"
    },
    "loglevel": {
      "type": "string",
      "enum": ["DEBUG", "VERBOSE", "INFO", "WARN", "ERROR"],
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
    "href": {
      "type": "string",
      "description": "API URL of the execution."
    },
    "permalink": {
      "type": "string",
      "description": "GUI URL of the execution."
    },
    "status": {
      "type": "string",
      "enum": [
        "running",
        "scheduled",
        "succeeded",
        "failed",
        "aborted",
        "timedout",
        "failed-with-retry",
        "other"
      ],
      "description": "running and scheduled are non-terminal; scheduled persists until the runAtTime of the execution. succeeded, failed, aborted, timedout, other are terminal. failed-with-retry is terminal for this execution: it failed and Rundeck starts a separate retry execution with its own id."
    },
    "customStatus": {
      "type": "string",
      "description": "The custom exit status string; present when status is other."
    },
    "executionType": {
      "type": "string"
    },
    "jobDeleted": {
      "type": "boolean"
    },
    "abortedby": {
      "type": ["string", "null"],
      "description": "Username that aborted the execution; present when aborted."
    },
    "retriedExecution": {
      "type": "object",
      "description": "id, href and permalink of the retry execution; present on failed-with-retry."
    },
    "project": {
      "type": "string"
    },
    "user": {
      "type": "string",
      "description": "Username of the user who started the execution."
    },
    "serverUUID": {
      "type": "string",
      "description": "UUID of the cluster member that ran the execution; present in cluster mode."
    },
    "date-started": {
      "type": "object",
      "properties": {
        "unixtime": {
          "type": "integer",
          "description": "Millisecond unix timestamp."
        },
        "date": {
          "type": "string",
          "description": "W3C dateTime, \"yyyy-MM-ddTHH:mm:ssZ\"."
        }
      }
    },
    "date-ended": {
      "type": "object",
      "description": "Absent until the execution is terminal.",
      "properties": {
        "unixtime": {
          "type": "integer",
          "description": "Millisecond unix timestamp."
        },
        "date": {
          "type": "string",
          "description": "W3C dateTime, \"yyyy-MM-ddTHH:mm:ssZ\"."
        }
      }
    },
    "job": {
      "type": "object",
      "properties": {
        "id": {
          "type": "string",
          "description": "Job UUID."
        },
        "href": {
          "type": "string"
        },
        "permalink": {
          "type": "string"
        },
        "averageDuration": {
          "type": "integer",
          "description": "Milliseconds; present when known."
        },
        "name": {
          "type": "string"
        },
        "group": {
          "type": "string"
        },
        "project": {
          "type": "string"
        },
        "description": {
          "type": "string"
        },
        "options": {
          "type": "object",
          "description": "Parsed option name to option value; present when an argstring value is set.",
          "additionalProperties": true
        }
      },
      "required": [
        "id",
        "name",
        "project"
      ]
    },
    "description": {
      "type": "string",
      "description": "Summary of the workflow of this execution."
    },
    "argstring": {
      "type": ["string", "null"],
      "description": "Option values in command-line form."
    },
    "successfulNodes": {
      "type": "array",
      "description": "Names of the nodes that succeeded.",
      "items": {
        "type": "string"
      }
    },
    "failedNodes": {
      "type": "array",
      "description": "Names of the nodes that failed.",
      "items": {
        "type": "string"
      }
    },
    "error": {
      "type": "boolean",
      "description": "true on an error response."
    },
    "errorCode": {
      "type": "string",
      "description": "Error identifier on an error response, e.g. \"api.error.item.doesnotexist\", \"unauthorized\"."
    },
    "message": {
      "type": "string",
      "description": "Human-readable error text on an error response."
    },
    "apiversion": {
      "type": "integer",
      "description": "API version that served an error response."
    }
  },
  "required": [
    "id",
    "href",
    "permalink",
    "status",
    "project",
    "user",
    "date-started",
    "description"
  ]
}
```

### 4.1.5 Implementation

| | |
|---|---|
| **Invocation** | `fetch_then_create` |
| **Monitoring** | `request_poll` |

```python
import time
from urllib.parse import quote

import requests

API = "/api/59"


def _check(r):
    # Rundeck answers both an unauthenticated call and a call the token's ACL
    # does not permit with 403 errorCode "unauthorized" — an operator must
    # issue a token or grant the ACL.
    if r.status_code == 403:
        raise Exception("halt", r.text)
    if r.status_code == 429 or r.status_code >= 500:
        raise Exception("release", r.text)
    return r


def _auth(cfg):
    return {"X-Rundeck-Auth-Token": cfg.api_token, "Accept": "application/json"}


def job_run(cfg, promise):
    args = promise.param["args"]
    job_id = quote(args["id"], safe="")
    token = sanitize(promise.id)

    info = _check(
        requests.get(
            f"{cfg.base_url}{API}/job/{job_id}/info",
            headers=_auth(cfg),
            timeout=10,
        )
    )
    if info.status_code == 404:
        return ("rejected", {"code": "job_not_found", "detail": info.json()["message"]})
    if info.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": info.json()["message"]})
    project = quote(info.json()["project"], safe="")

    # Locate: every job accepts undeclared options, and optionFilter matches
    # option values — an execution started by an earlier delivery is
    # recoverable by its stamped rescorr option.
    q = _check(
        requests.get(
            f"{cfg.base_url}{API}/project/{project}/executions",
            headers=_auth(cfg),
            params={"optionFilter": f"-rescorr {token}"},
            timeout=10,
        )
    )
    if q.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": q.json()["message"]})
    hits = q.json()["executions"]

    if hits:
        e = min(hits, key=lambda x: x["id"])  # deterministic under races
    else:
        body = {k: v for k, v in args.items() if k != "id"}
        if "argString" in body and "options" not in body:
            body["argString"] = f"{body['argString']} -rescorr {token}"
        else:
            body["options"] = {**body.get("options", {}), "rescorr": token}
        r = _check(
            requests.post(
                f"{cfg.base_url}{API}/job/{job_id}/run",
                headers=_auth(cfg) | {"Content-Type": "application/json"},
                json=body,
                timeout=10,
            )
        )
        if r.status_code == 404:
            return ("rejected", {"code": "job_not_found", "detail": r.json()["message"]})
        if r.status_code == 409:
            # api.error.execution.conflict: the job is already running
            # (multipleExecutions off) or executions are disabled — clears
            # with time.
            raise Exception("release", r.text)
        if r.status_code >= 400:
            return ("rejected", {"code": "invalid_request", "detail": r.json()["message"]})
        e = r.json()

    while e["status"] in ("running", "scheduled"):
        if time.time() * 1000 >= promise.timeout_at:
            raise Exception("release", "promise timed out")
        time.sleep(
            min(
                cfg.poll.total_seconds(),
                (promise.timeout_at - time.time() * 1000) / 1000,
            )
        )
        g = _check(
            requests.get(
                f"{cfg.base_url}{API}/execution/{e['id']}",
                headers=_auth(cfg),
                timeout=10,
            )
        )
        if g.status_code == 404:
            return ("rejected", {"code": "deleted"})
        if g.status_code >= 400:
            return ("rejected", {"code": "invalid_request", "detail": g.json()["message"]})
        e = g.json()

    if e["status"] == "succeeded":
        keys = (
            "id",
            "href",
            "permalink",
            "status",
            "project",
            "user",
            "date-started",
            "date-ended",
            "job",
            "description",
            "argstring",
            "successfulNodes",
            "failedNodes",
        )
        return ("resolved", {k: e[k] for k in keys if k in e})  # the 4.1.2 Resolved mapping
    if e["status"] == "failed":
        return ("rejected", {"code": "execution_failed", "detail": e})
    if e["status"] == "aborted":
        return ("rejected", {"code": "execution_aborted", "detail": e})
    if e["status"] == "timedout":
        return ("rejected", {"code": "execution_timedout", "detail": e})
    if e["status"] == "failed-with-retry":
        # The retry runs as a separate execution with its own id; this one is over.
        return ("rejected", {"code": "execution_failed_with_retry", "detail": e})
    return ("rejected", {"code": "execution_other", "detail": e})
```


### 4.2 job.get

| | |
|---|---|
| **Documentation** | [Getting a Job Definition](https://docs.rundeck.com/docs/api/#getting-a-job-definition) |

```json
{ "func": "job.get", "args": { ... } }
```

### 4.2.1 Promise Param Schema

```json
{
  "description": "Export one job definition in job-json format, including \"options\" — the declared option names, defaults, allowed values and regexes, which are the valid keys and values of job.run's options for this job. Rejects not_found if the job id is unknown, invalid_request on a malformed request.",
  "type": "object",
  "properties": {
    "id": {
      "type": "string",
      "description": "Job UUID."
    }
  },
  "required": [
    "id"
  ]
}
```

### 4.2.2 Promise Value Schema

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
      "enum": ["not_found", "invalid_request"]
    },
    "detail": {
      "description": "invalid_request: = response.body.message of the 4xx; not_found: absent"
    }
  },
  "required": ["code"]
}
```

### 4.2.3 Integration Request

```
GET /api/59/job/{promise.param.id} → 200
Accept: application/json
```

### 4.2.4 Integration Response

```json
{
  "type": "array",
  "description": "job-json format; one element for a single job export.",
  "items": {
    "type": "object",
    "properties": {
      "id": {
        "type": "string",
        "description": "Job UUID."
      },
      "uuid": {
        "type": "string",
        "description": "Job UUID."
      },
      "name": {
        "type": "string"
      },
      "group": {
        "type": "string",
        "description": "Job group path."
      },
      "description": {
        "type": "string",
        "description": "First line is the short description; remaining lines are Markdown."
      },
      "loglevel": {
        "type": "string",
        "enum": ["DEBUG", "VERBOSE", "INFO", "WARN", "ERROR"]
      },
      "multipleExecutions": {
        "type": "boolean",
        "description": "true if the job may have more than one execution at once."
      },
      "timeout": {
        "type": "string",
        "description": "Maximum runtime before the execution is stopped, e.g. \"120\" (seconds) or \"6h 30m\"; may be an option reference such as \"${option.timeout}\"."
      },
      "retry": {
        "description": "Maximum retries as an integer or option reference, or an object with \"retry\" and \"delay\"."
      },
      "options": {
        "type": "array",
        "description": "Declared options of the job; the valid keys of job.run's options.",
        "items": {
          "type": "object",
          "properties": {
            "name": {
              "type": "string",
              "description": "Option name."
            },
            "description": {
              "type": "string"
            },
            "value": {
              "type": "string",
              "description": "Default value."
            },
            "values": {
              "type": "array",
              "description": "Allowed values.",
              "items": {
                "type": "string"
              }
            },
            "valuesUrl": {
              "type": "string",
              "description": "URL returning a JSON set of allowed values."
            },
            "required": {
              "type": "boolean"
            },
            "enforced": {
              "type": "boolean",
              "description": "true if the value must be one of values."
            },
            "regex": {
              "type": "string",
              "description": "Regular expression the value must match."
            },
            "multivalued": {
              "type": "boolean"
            },
            "delimiter": {
              "type": "string",
              "description": "Conjoins multiple values; set when multivalued is true."
            },
            "secure": {
              "type": "boolean",
              "description": "true for a secure input option."
            },
            "valueExposed": {
              "type": "boolean",
              "description": "true if a secure option value is exposed to scripts."
            },
            "storagePath": {
              "type": "string",
              "description": "Key storage path holding a secure option's default value."
            },
            "isDate": {
              "type": "boolean"
            },
            "dateFormat": {
              "type": "string",
              "description": "momentjs format used by the GUI."
            }
          },
          "required": [
            "name"
          ]
        }
      },
      "schedule": {
        "type": "object",
        "description": "Crontab string under \"crontab\", or explicit time, weekday, month, year components."
      },
      "sequence": {
        "type": "object",
        "description": "Workflow sequence: \"commands\" plus \"keepgoing\" and \"strategy\"."
      },
      "nodefilters": {
        "type": "object",
        "description": "Node filter definition of the job."
      },
      "nodeFilterEditable": {
        "type": "boolean",
        "description": "true if node filters may be overridden at run time."
      },
      "notification": {
        "type": "object"
      },
      "user": {
        "type": ["string", "null"],
        "description": "Username of the job creator; null for jobs created before 5.17.0."
      },
      "createdBy": {
        "type": ["string", "null"]
      },
      "lastModifiedBy": {
        "type": ["string", "null"]
      },
      "created": {
        "type": ["string", "null"],
        "description": "ISO-8601"
      },
      "lastModified": {
        "type": ["string", "null"],
        "description": "ISO-8601"
      }
    },
    "required": [
      "name",
      "description",
      "loglevel",
      "sequence"
    ]
  }
}
```

### 4.2.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def job_get(cfg, promise):
    job_id = quote(promise.param["args"]["id"], safe="")

    r = _check(
        requests.get(
            f"{cfg.base_url}{API}/job/{job_id}",
            headers=_auth(cfg),
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found"})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.json()["message"]})
    return ("resolved", r.json())
```


### 4.3 job.list

| | |
|---|---|
| **Documentation** | [Listing Jobs](https://docs.rundeck.com/docs/api/#listing-jobs) |

```json
{ "func": "job.list", "args": { ... } }
```

### 4.3.1 Promise Param Schema

```json
{
  "description": "List the jobs of one project, for discovering job ids and composing job.run calls. Rejects not_found if the project does not exist, invalid_request on a malformed filter.",
  "type": "object",
  "properties": {
    "project": {
      "type": "string",
      "description": "Project name."
    },
    "idlist": {
      "type": "string",
      "description": "Comma-separated list of job UUIDs to include."
    },
    "groupPath": {
      "type": "string",
      "description": "Group or partial group path to include; \"*\" (all groups) when omitted, \"-\" matches top-level jobs only. Cannot be combined with groupPathExact."
    },
    "groupPathExact": {
      "type": "string",
      "description": "Exact group path to match; \"-\" matches top-level jobs only."
    },
    "jobFilter": {
      "type": "string",
      "description": "Substring match on the job name."
    },
    "jobExactFilter": {
      "type": "string",
      "description": "Exact job name to match."
    },
    "scheduledFilter": {
      "type": "boolean",
      "description": "Restrict to scheduled or to unscheduled jobs."
    },
    "serverNodeUUIDFilter": {
      "type": "string",
      "description": "In cluster mode, select scheduled jobs assigned to this server UUID."
    },
    "tags": {
      "type": "string",
      "description": "Tag or comma-separated list of tags the job must carry."
    },
    "max": {
      "type": "integer",
      "minimum": 0,
      "description": "Page size."
    },
    "offset": {
      "type": "integer",
      "minimum": 0,
      "description": "0-indexed offset of the first result; used with max."
    }
  },
  "required": [
    "project"
  ]
}
```

### 4.3.2 Promise Value Schema

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
      "enum": ["not_found", "invalid_request"]
    },
    "detail": {
      "description": "invalid_request: = response.body.message of the 4xx; not_found: absent"
    }
  },
  "required": ["code"]
}
```

### 4.3.3 Integration Request

```
GET /api/59/project/{promise.param.project}/jobs{?idlist,groupPath,groupPathExact,jobFilter,jobExactFilter,scheduledFilter,serverNodeUUIDFilter,tags,max,offset = promise.param.*} → 200
Accept: application/json
```

### 4.3.4 Integration Response

```json
{
  "type": "array",
  "items": {
    "type": "object",
    "properties": {
      "id": {
        "type": "string",
        "description": "Job UUID; the id job.run takes."
      },
      "name": {
        "type": "string"
      },
      "group": {
        "type": ["string", "null"],
        "description": "Group path; absent or null for top-level jobs."
      },
      "project": {
        "type": "string"
      },
      "description": {
        "type": "string"
      },
      "href": {
        "type": "string",
        "description": "API URL of the job."
      },
      "permalink": {
        "type": "string",
        "description": "GUI URL of the job."
      },
      "scheduled": {
        "type": "boolean",
        "description": "true if the job has a schedule."
      },
      "scheduleEnabled": {
        "type": "boolean",
        "description": "true if the job's schedule is enabled."
      },
      "enabled": {
        "type": "boolean",
        "description": "false when executions are disabled for the job."
      },
      "serverNodeUUID": {
        "type": "string",
        "description": "UUID of the schedule owner server; present in cluster mode."
      },
      "serverOwner": {
        "type": "boolean",
        "description": "true if the target server owns the schedule; present in cluster mode."
      },
      "createdBy": {
        "type": ["string", "null"],
        "description": "null for jobs created before 5.17.0."
      },
      "lastModifiedBy": {
        "type": ["string", "null"]
      },
      "created": {
        "type": ["string", "null"],
        "description": "ISO-8601"
      },
      "lastModified": {
        "type": ["string", "null"],
        "description": "ISO-8601"
      }
    },
    "required": [
      "id",
      "name",
      "project",
      "href",
      "permalink",
      "scheduled",
      "scheduleEnabled",
      "enabled"
    ]
  }
}
```

### 4.3.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def job_list(cfg, promise):
    args = promise.param["args"]
    project = quote(args["project"], safe="")

    # Pagination is the caller's loop (one promise per page), not ours.
    params = {k: v for k, v in args.items() if k != "project"}
    r = _check(
        requests.get(
            f"{cfg.base_url}{API}/project/{project}/jobs",
            headers=_auth(cfg),
            params=params,
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found"})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.json()["message"]})
    return ("resolved", r.json())
```


### 4.4 execution.get

| | |
|---|---|
| **Documentation** | [Execution Info](https://docs.rundeck.com/docs/api/#execution-info) |

```json
{ "func": "execution.get", "args": { ... } }
```

### 4.4.1 Promise Param Schema

```json
{
  "description": "Read one execution record. Rejects not_found if the execution does not exist, invalid_request on a malformed request. A plain read — not the completion mechanism; job.run observes independently.",
  "type": "object",
  "properties": {
    "id": {
      "type": "string",
      "description": "Execution id."
    }
  },
  "required": [
    "id"
  ]
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
      "description": "invalid_request: = response.body.message of the 4xx; not_found: absent"
    }
  },
  "required": ["code"]
}
```

### 4.4.3 Integration Request

```
GET /api/59/execution/{promise.param.id} → 200
Accept: application/json
```

### 4.4.4 Integration Response

Same as 4.1.4.

### 4.4.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def execution_get(cfg, promise):
    execution_id = quote(promise.param["args"]["id"], safe="")

    r = _check(
        requests.get(
            f"{cfg.base_url}{API}/execution/{execution_id}",
            headers=_auth(cfg),
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found"})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.json()["message"]})
    return ("resolved", r.json())
```


### 4.5 executionoutput.get

| | |
|---|---|
| **Documentation** | [Execution Output](https://docs.rundeck.com/docs/api/#execution-output) |

```json
{ "func": "executionoutput.get", "args": { ... } }
```

### 4.5.1 Promise Param Schema

```json
{
  "description": "Read the log output an execution produced — the result a job run publishes. The execution may be running or finished; \"completed\" reports whether the returned entries include all data available at this offset, \"execCompleted\" whether the execution itself has finished. Rejects not_found if the execution does not exist, invalid_request on a malformed request. A plain read — not the completion mechanism; job.run observes independently.",
  "type": "object",
  "properties": {
    "id": {
      "type": "string",
      "description": "Execution id."
    },
    "offset": {
      "type": "integer",
      "minimum": 0,
      "description": "Byte offset in the log file to read from; 0 is the beginning. Opaque location, not a count of log text."
    },
    "lastlines": {
      "type": "integer",
      "minimum": 0,
      "description": "Number of lines to retrieve from the end of the output; overrides offset."
    },
    "lastmod": {
      "type": "integer",
      "description": "Millisecond epoch timestamp; return results only if the log changed since then or more data exists at offset."
    },
    "maxlines": {
      "type": "integer",
      "minimum": 0,
      "description": "Maximum number of lines to retrieve forward from offset."
    },
    "compacted": {
      "type": "boolean",
      "description": "Return entries in compacted form: each entry carries only the values that changed from the previous entry."
    },
    "nodename": {
      "type": "string",
      "description": "Filter log entries to one node."
    },
    "stepctx": {
      "type": "string",
      "description": "Filter log entries to a step context, e.g. \"1\" or \"1/2/3\"."
    }
  },
  "required": [
    "id"
  ]
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
      "description": "invalid_request: = response.body.message of the 4xx; not_found: absent"
    }
  },
  "required": ["code"]
}
```

### 4.5.3 Integration Request

```
GET /api/59/execution/{promise.param.id}/output{?offset,lastlines,lastmod,maxlines,compacted,nodename,stepctx = promise.param.*} → 200
Accept: application/json
```

### 4.5.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "id": {
      "type": "string",
      "description": "Execution id."
    },
    "offset": {
      "type": ["integer", "string"],
      "description": "Byte offset to read for the next set of data."
    },
    "completed": {
      "type": "boolean",
      "description": "true if the returned entries include all data available for this request."
    },
    "execCompleted": {
      "type": "boolean",
      "description": "true if the execution has finished."
    },
    "execState": {
      "type": "string",
      "enum": ["scheduled", "running", "succeeded", "failed", "aborted"],
      "description": "Execution state as reported by the log endpoint."
    },
    "execDuration": {
      "type": "integer",
      "description": "Milliseconds the execution has run."
    },
    "hasFailedNodes": {
      "type": "boolean",
      "description": "true if a list of failed nodes was recorded."
    },
    "lastModified": {
      "type": "string",
      "description": "Millisecond timestamp of the last modification of the log file, as a string."
    },
    "totalSize": {
      "type": "integer",
      "description": "Total bytes available in the output file."
    },
    "percentLoaded": {
      "type": "number",
      "description": "Percentage of the output loaded by this request."
    },
    "unmodified": {
      "type": "boolean",
      "description": "true when lastmod was sent and the file had not changed."
    },
    "empty": {
      "type": "boolean",
      "description": "true when the log file does not exist or is empty."
    },
    "message": {
      "type": "string",
      "description": "Why no entries were returned; also the error text on an error response."
    },
    "error": {
      "description": "Error text on this endpoint; boolean true on a Rundeck error response body."
    },
    "compacted": {
      "type": ["boolean", "string"],
      "description": "true when compacted form was requested and used."
    },
    "compactedAttr": {
      "type": "string",
      "description": "Log entry key used for fully compacted entries."
    },
    "filter": {
      "type": "object",
      "properties": {
        "nodename": {
          "type": "string"
        },
        "stepctx": {
          "type": "string"
        }
      }
    },
    "entries": {
      "type": "array",
      "description": "Log entries. In compacted form an entry carries only changed values, a null value means the key is dropped, and from API v59 every entry is an object.",
      "items": {
        "type": "object",
        "properties": {
          "time": {
            "type": "string",
            "description": "\"HH:MM:SS\""
          },
          "absolute_time": {
            "type": "string",
            "description": "\"yyyy-MM-dd'T'HH:mm:ssZ\""
          },
          "level": {
            "type": "string",
            "enum": ["ERROR", "WARN", "NORMAL", "VERBOSE", "DEBUG", "OTHER"]
          },
          "log": {
            "type": "string",
            "description": "The log message."
          },
          "user": {
            "type": "string"
          },
          "command": {
            "type": ["string", "null"],
            "description": "Workflow command context string."
          },
          "node": {
            "type": ["string", "null"]
          },
          "stepctx": {
            "type": ["string", "null"],
            "description": "Step context such as \"1\" or \"1/2/3\"."
          },
          "metadata": {
            "type": "object",
            "description": "Extra metadata for the entry.",
            "additionalProperties": true
          }
        }
      }
    }
  },
  "required": [
    "id",
    "entries"
  ]
}
```

### 4.5.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def executionoutput_get(cfg, promise):
    args = promise.param["args"]
    execution_id = quote(args["id"], safe="")

    # Following the log to its end is the caller's loop (one promise per
    # offset), not ours. requests renders Python booleans as "True"/"False";
    # Rundeck's compacted parameter is case-sensitive.
    params = {
        k: (str(v).lower() if isinstance(v, bool) else v)
        for k, v in args.items()
        if k != "id"
    }
    r = _check(
        requests.get(
            f"{cfg.base_url}{API}/execution/{execution_id}/output",
            headers=_auth(cfg),
            params=params,
            timeout=10,
        )
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found"})
    if r.status_code >= 400:
        return ("rejected", {"code": "invalid_request", "detail": r.json()["message"]})
    return ("resolved", r.json())
```


## 5. Test

### 5.1 Base Image

```dockerfile
FROM rundeck/rundeck:6.1.0
USER root
RUN apt-get update \
 && apt-get install -y --no-install-recommends python3 python3-requests curl \
 && rm -rf /var/lib/apt/lists/*
USER rundeck
ENV RUNDECK_GRAILS_URL=http://localhost:4440
```

### 5.2 Run

```sh
docker rm -f plugin-rundeck-test >/dev/null 2>&1 || true
docker build -t plugin-rundeck-test spec/
docker run -d --name plugin-rundeck-test -p 4440:4440 plugin-rundeck-test

while :; do
  rm -f /tmp/plugin-rundeck-cookies
  curl -s -L -c /tmp/plugin-rundeck-cookies -b /tmp/plugin-rundeck-cookies -o /dev/null \
    -X POST http://localhost:4440/j_security_check -d j_username=admin -d j_password=admin
  RUNDECK_API_TOKEN=$(curl -s -c /tmp/plugin-rundeck-cookies -b /tmp/plugin-rundeck-cookies \
    -X POST http://localhost:4440/api/59/tokens/admin \
    -H 'Content-Type: application/json' -H 'Accept: application/json' \
    -d '{"roles":["admin"],"duration":"0"}' \
    | python3 -c 'import json,sys; print(json.load(sys.stdin).get("token",""))' 2>/dev/null)
  if [ -n "$RUNDECK_API_TOKEN" ] \
     && curl -sf -o /dev/null -H "X-Rundeck-Auth-Token: $RUNDECK_API_TOKEN" \
        http://localhost:4440/api/59/system/info; then
    break
  fi
  sleep 5
done

export RUNDECK_BASE_URL=http://localhost:4440
export RUNDECK_API_TOKEN
```
