# Apache Airflow

| | |
|---|---|
| **API** | `{base_url}/api/v2` |
| **Idempotency** | `dag_run_id` in the trigger body — caller-supplied, unique per `dag_id`, no expiry; a repeat with the same id returns `409`. Must match `^[A-Za-z0-9_.~:+-]+$` (`[scheduler] allowed_run_id_pattern`) and must not contain `..` |
| **Reviewed by** | Claude Opus, 2026-08-26 |

**Notes**

| | |
|---|---|
| **OpenAPI** | `https://airflow.apache.org/docs/apache-airflow/stable/_static/dot-dot/src/airflow/api_fastapi/core_api/openapi/v2-rest-api-generated.yaml` |
| **Self-hosted** | yes — §5 |

## 1. Address

```
airflow://[{instance}]           # omitted instance = "default"
```

## 2. Configuration

```toml
[airflow.{instance}]             # [airflow] = [airflow.default]
```

| key | type | default | example |
|---|---|---|---|
| `base_url` | `String` | | `https://airflow.acme.com` |
| `username` | `String` | | `resonate` |
| `password` | `String` | | `…` |
| `poll` | `Duration` | `30s` | `30s` |

## 3. Authentication & Authorization

| | |
|---|---|
| **Documentation** | [Public API](https://airflow.apache.org/docs/apache-airflow/stable/security/api.html) |
| **Probe** | `GET /api/v2/dags?limit=1` → `200` |

```
POST /auth/token  {"username": "{username}", "password": "{password}"}  → 201 {"access_token": …}
Authorization: Bearer {access_token}
```

## 4. Operations

### 4.1 dagrun.trigger

| | |
|---|---|
| **Documentation** | [Trigger Dag Run](https://airflow.apache.org/docs/apache-airflow/stable/stable-rest-api-ref.html#/DagRun/trigger_dag_run) |

```json
{ "func": "dagrun.trigger", "args": { ... } }
```

### 4.1.1 Promise Param Schema

```json
{
  "description": "Trigger a manual Dag run and observe it to a terminal state. Resolves when the run reaches state \"success\"; rejects run_failed on state \"failed\", dag_not_found if the dag_id is unknown or stale, invalid_request if the Dag has import errors, forbids manual runs, or the body fails Dag param validation, conflict if the trigger collides with an existing run's unique constraints (e.g. an existing run with the same logical_date), and deleted if the run is removed before finishing. Duration is the Dag's own runtime — seconds to hours; a paused Dag accepts the trigger but the run stays \"queued\" until unpaused, so size timeoutAt accordingly.",
  "type": "object",
  "properties": {
    "dag_id": {
      "type": "string",
      "description": "Dag to trigger. Enumerate via dag.list."
    },
    "conf": {
      "type": "object",
      "description": "Run configuration. Validated against the Dag's declared params — see dag.get.",
      "additionalProperties": true
    },
    "logical_date": {
      "type": ["string", "null"],
      "format": "date-time",
      "description": "Timezone-aware ISO 8601. null = a run with no logical date. Defaults to null when omitted."
    },
    "data_interval_start": {
      "type": "string",
      "format": "date-time",
      "description": "Timezone-aware ISO 8601. Must be supplied together with data_interval_end. Ignored when logical_date is null."
    },
    "data_interval_end": {
      "type": "string",
      "format": "date-time",
      "description": "Timezone-aware ISO 8601. Must be supplied together with data_interval_start."
    },
    "run_after": {
      "type": "string",
      "format": "date-time",
      "description": "Timezone-aware ISO 8601; earliest scheduling time. Defaults to now."
    },
    "note": {
      "type": "string",
      "description": "Free-text note attached to the run."
    },
    "partition_key": {
      "type": "string",
      "description": "Validated against the Dag's timetable; resolves partition_date."
    }
  },
  "required": [
    "dag_id"
  ],
  "additionalProperties": false
}
```

### 4.1.2 Promise Value Schema

#### Resolved

```json
{
  "type": "object",
  "properties": {
    "dag_id": {
      "type": "string",
      "description": "= response.body.dag_id"
    },
    "dag_run_id": {
      "type": "string",
      "description": "= response.body.dag_run_id (equals sanitize(promise.id))"
    },
    "state": {
      "type": "string",
      "description": "= response.body.state"
    },
    "logical_date": {
      "type": ["string", "null"],
      "description": "= response.body.logical_date"
    },
    "run_after": {
      "type": "string",
      "description": "= response.body.run_after"
    },
    "start_date": {
      "type": ["string", "null"],
      "description": "= response.body.start_date"
    },
    "end_date": {
      "type": ["string", "null"],
      "description": "= response.body.end_date"
    },
    "duration": {
      "type": ["number", "null"],
      "description": "= response.body.duration (seconds)"
    },
    "conf": {
      "type": ["object", "null"],
      "description": "= response.body.conf"
    },
    "run_type": {
      "type": "string",
      "description": "= response.body.run_type"
    },
    "note": {
      "type": ["string", "null"],
      "description": "= response.body.note"
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
      "enum": ["dag_not_found", "invalid_request", "conflict", "run_failed", "deleted"]
    },
    "detail": {
      "description": "dag_not_found / invalid_request / conflict: = response.body.detail of the 400/404/409/422; run_failed: = response.body (the terminal Dag run object); deleted: absent"
    }
  },
  "required": ["code"]
}
```

### 4.1.3 Integration Request

```
POST /api/v2/dags/{promise.param.dag_id}/dagRuns → 200
Authorization: Bearer {access_token}
```

```json
{
  "type": "object",
  "properties": {
    "dag_run_id": {
      "type": "string",
      "description": "= sanitize(promise.id)"
    },
    "conf": {
      "type": "object",
      "description": "= promise.param.conf"
    },
    "logical_date": {
      "type": ["string", "null"],
      "description": "= promise.param.logical_date (key always present; null when omitted)"
    },
    "data_interval_start": {
      "type": "string",
      "description": "= promise.param.data_interval_start"
    },
    "data_interval_end": {
      "type": "string",
      "description": "= promise.param.data_interval_end"
    },
    "run_after": {
      "type": "string",
      "description": "= promise.param.run_after"
    },
    "note": {
      "type": "string",
      "description": "= promise.param.note"
    },
    "partition_key": {
      "type": "string",
      "description": "= promise.param.partition_key"
    }
  },
  "required": [
    "logical_date"
  ],
  "additionalProperties": false
}
```

### 4.1.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "dag_run_id": {
      "type": "string",
      "description": "External identity, together with dag_id."
    },
    "dag_id": {
      "type": "string"
    },
    "dag_display_name": {
      "type": "string"
    },
    "state": {
      "type": "string",
      "enum": ["queued", "running", "success", "failed"],
      "description": "success and failed are terminal; clearing a run returns it to queued."
    },
    "run_type": {
      "type": "string",
      "enum": ["backfill", "scheduled", "manual", "operator_triggered", "asset_triggered", "asset_materialization"]
    },
    "triggered_by": {
      "type": ["string", "null"],
      "enum": ["cli", "operator", "rest_api", "ui", "test", "timetable", "asset", "backfill"]
    },
    "triggering_user_name": {
      "type": ["string", "null"]
    },
    "logical_date": {
      "type": ["string", "null"],
      "description": "ISO 8601; null for runs triggered without one."
    },
    "queued_at": {
      "type": ["string", "null"],
      "description": "ISO 8601"
    },
    "start_date": {
      "type": ["string", "null"],
      "description": "ISO 8601; null until the run starts."
    },
    "end_date": {
      "type": ["string", "null"],
      "description": "ISO 8601; null until the run is terminal."
    },
    "duration": {
      "type": ["number", "null"],
      "description": "Seconds; null until the run is terminal."
    },
    "data_interval_start": {
      "type": ["string", "null"],
      "description": "ISO 8601"
    },
    "data_interval_end": {
      "type": ["string", "null"],
      "description": "ISO 8601"
    },
    "run_after": {
      "type": "string",
      "description": "ISO 8601"
    },
    "last_scheduling_decision": {
      "type": ["string", "null"],
      "description": "ISO 8601"
    },
    "conf": {
      "type": ["object", "null"],
      "description": "Echo of the submitted conf."
    },
    "note": {
      "type": ["string", "null"]
    },
    "partition_key": {
      "type": ["string", "null"]
    },
    "partition_date": {
      "type": ["string", "null"],
      "description": "ISO 8601"
    },
    "bundle_version": {
      "type": ["string", "null"]
    },
    "dag_versions": {
      "type": "array",
      "description": "Dag versions this run executed against."
    }
  },
  "required": [
    "dag_run_id",
    "dag_id",
    "state",
    "run_type",
    "run_after",
    "logical_date",
    "conf",
    "note"
  ]
}
```

### 4.1.5 Implementation

| | |
|---|---|
| **Invocation** | `create_idempotent` |
| **Monitoring** | `request_poll` |

```python
import time
from urllib.parse import quote

import requests


def _check(r):
    # 403: authenticated but not permitted — an operator must act.
    if r.status_code == 403:
        raise Exception("halt", r.text)
    if r.status_code >= 400:
        raise Exception("release", r.text)


def _token(cfg):
    r = requests.post(
        f"{cfg.base_url}/auth/token",
        json={"username": cfg.username, "password": cfg.password},
        timeout=10,
    )
    if r.status_code in (401, 403):
        raise Exception("halt", f"credentials rejected: {r.text}")
    _check(r)
    return {"Authorization": f"Bearer {r.json()['access_token']}"}


def dagrun_trigger(cfg, promise):
    api = f"{cfg.base_url}/api/v2"
    auth = _token(cfg)
    args = promise.param["args"]
    dag_id = quote(args["dag_id"], safe="")

    body = {k: v for k, v in args.items() if k != "dag_id"}
    body["logical_date"] = args.get("logical_date")  # required key, may be null

    # (dag_id, dag_run_id) is unique with no expiry window, so a run created
    # by an earlier attempt is always recoverable by GET.
    r = requests.get(f"{api}/dags/{dag_id}/dagRuns/{sanitize(promise.id)}", headers=auth, timeout=10)
    if r.status_code == 404:
        r = requests.post(
            f"{api}/dags/{dag_id}/dagRuns",
            headers=auth,
            json=body | {"dag_run_id": sanitize(promise.id)},
            timeout=10,
        )
        if r.status_code == 404:
            return ("rejected", {"code": "dag_not_found", "detail": r.json()["detail"]})
        # 400: import errors, manual runs not allowed, Dag param validation,
        # or a run_id outside allowed_run_id_pattern. 422: body schema.
        if r.status_code in (400, 422):
            return ("rejected", {"code": "invalid_request", "detail": r.json()["detail"]})
        if r.status_code == 409:
            # 409 covers every unique constraint on the run row, not only
            # (dag_id, run_id) — e.g. a (dag_id, logical_date) collision
            # with an existing run. Only our own run id existing means a
            # previous attempt created it.
            g = requests.get(
                f"{api}/dags/{dag_id}/dagRuns/{sanitize(promise.id)}",
                headers=auth,
                timeout=10,
            )
            if g.status_code == 404:
                return ("rejected", {"code": "conflict", "detail": r.json()["detail"]})
            _check(g)
        else:
            _check(r)
    else:
        _check(r)

    while True:
        if time.time() * 1000 >= promise.timeout_at:
            raise Exception("release", "promise timed out")
        r = requests.get(f"{api}/dags/{dag_id}/dagRuns/{sanitize(promise.id)}", headers=auth, timeout=10)
        if r.status_code == 401:
            # JWTs expire after [api_auth] jwt_expiration_time (default 86400s).
            auth = _token(cfg)
            continue
        if r.status_code == 404:
            return ("rejected", {"code": "deleted"})
        _check(r)
        run = r.json()
        if run["state"] in ("success", "failed"):
            break
        # A paused Dag accepts the trigger but the scheduler never queues its
        # task instances: the run sits in "queued" until the Dag is unpaused.
        time.sleep(cfg.poll.total_seconds())

    if run["state"] == "success":
        keys = (
            "dag_id",
            "dag_run_id",
            "state",
            "logical_date",
            "run_after",
            "start_date",
            "end_date",
            "duration",
            "conf",
            "run_type",
            "note",
        )
        return ("resolved", {k: run[k] for k in keys})  # the 4.1.2 Resolved mapping
    return ("rejected", {"code": "run_failed", "detail": run})
```


### 4.2 dagrun.get

| | |
|---|---|
| **Documentation** | [Get Dag Run](https://airflow.apache.org/docs/apache-airflow/stable/stable-rest-api-ref.html#/DagRun/get_dag_run) |

```json
{ "func": "dagrun.get", "args": { ... } }
```

### 4.2.1 Promise Param Schema

```json
{
  "description": "Read one Dag run. Rejects not_found if the run or Dag does not exist. A plain read — not the completion mechanism; dagrun.trigger observes independently.",
  "type": "object",
  "properties": {
    "dag_id": {
      "type": "string"
    },
    "dag_run_id": {
      "type": "string",
      "description": "Run id."
    }
  },
  "required": [
    "dag_id",
    "dag_run_id"
  ]
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
      "description": "invalid_request: = response.body.detail of the 400/422; not_found: absent"
    }
  },
  "required": ["code"]
}
```

### 4.2.3 Integration Request

```
GET /api/v2/dags/{promise.param.dag_id}/dagRuns/{promise.param.dag_run_id} → 200
Authorization: Bearer {access_token}
```

### 4.2.4 Integration Response

Same as 4.1.4.

### 4.2.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def dagrun_get(cfg, promise):
    api = f"{cfg.base_url}/api/v2"
    auth = _token(cfg)
    args = promise.param["args"]

    dag_id = quote(args["dag_id"], safe="")
    dag_run_id = quote(args["dag_run_id"], safe="")
    r = requests.get(
        f"{api}/dags/{dag_id}/dagRuns/{dag_run_id}",
        headers=auth,
        timeout=10,
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found"})
    if r.status_code in (400, 422):
        return ("rejected", {"code": "invalid_request", "detail": r.json()["detail"]})
    _check(r)
    return ("resolved", r.json())
```


### 4.3 taskinstance.list

| | |
|---|---|
| **Documentation** | [Get Task Instances](https://airflow.apache.org/docs/apache-airflow/stable/stable-rest-api-ref.html#/Task%20Instance/get_task_instances) |

```json
{ "func": "taskinstance.list", "args": { ... } }
```

### 4.3.1 Promise Param Schema

```json
{
  "description": "List the task instances of one Dag run — the per-task breakdown behind a run's state, including which task failed. Rejects not_found if the Dag or run does not exist, invalid_request on a malformed filter.",
  "type": "object",
  "properties": {
    "dag_id": {
      "type": "string"
    },
    "dag_run_id": {
      "type": "string",
      "description": "Run id."
    },
    "task_id": {
      "type": "string",
      "description": "Restrict to one task."
    },
    "state": {
      "type": "array",
      "description": "Restrict to these task instance states.",
      "items": {
        "type": "string",
        "enum": [
          "removed",
          "scheduled",
          "queued",
          "running",
          "success",
          "restarting",
          "failed",
          "up_for_retry",
          "up_for_reschedule",
          "upstream_failed",
          "skipped",
          "deferred",
          "awaiting_input"
        ]
      }
    },
    "map_index": {
      "type": "array",
      "description": "Restrict to these map indexes of a dynamically mapped task.",
      "items": {
        "type": "integer"
      }
    },
    "try_number": {
      "type": "array",
      "items": {
        "type": "integer"
      }
    },
    "limit": {
      "type": "integer",
      "minimum": 0,
      "description": "Page size; 50 when omitted."
    },
    "offset": {
      "type": "integer",
      "minimum": 0,
      "description": "0 when omitted."
    },
    "order_by": {
      "type": "array",
      "description": "Sort attributes; prefix with \"-\" for descending. Supported: id, state, duration, start_date, end_date, map_index, try_number, logical_date, run_after, data_interval_start, data_interval_end, rendered_map_index, operator. [\"map_index\"] when omitted.",
      "items": {
        "type": "string"
      }
    }
  },
  "required": [
    "dag_id",
    "dag_run_id"
  ]
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
      "enum": ["not_found", "invalid_request"]
    },
    "detail": {
      "description": "invalid_request: = response.body.detail of the 400/422; not_found: absent"
    }
  },
  "required": ["code"]
}
```

### 4.3.3 Integration Request

```
GET /api/v2/dags/{promise.param.dag_id}/dagRuns/{promise.param.dag_run_id}/taskInstances{?task_id,state,map_index,try_number,limit,offset,order_by = promise.param.*} → 200
Authorization: Bearer {access_token}
```

### 4.3.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "task_instances": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "id": {
            "type": "string",
            "description": "UUID of the task instance."
          },
          "task_id": {
            "type": "string"
          },
          "dag_id": {
            "type": "string"
          },
          "dag_run_id": {
            "type": "string"
          },
          "map_index": {
            "type": "integer",
            "description": "-1 for non-mapped tasks."
          },
          "state": {
            "type": ["string", "null"],
            "enum": [
              "removed",
              "scheduled",
              "queued",
              "running",
              "success",
              "restarting",
              "failed",
              "up_for_retry",
              "up_for_reschedule",
              "upstream_failed",
              "skipped",
              "deferred",
              "awaiting_input",
              null
            ]
          },
          "try_number": {
            "type": "integer"
          },
          "max_tries": {
            "type": "integer"
          },
          "operator": {
            "type": ["string", "null"]
          },
          "operator_name": {
            "type": ["string", "null"]
          },
          "task_display_name": {
            "type": "string"
          },
          "start_date": {
            "type": ["string", "null"],
            "description": "ISO 8601"
          },
          "end_date": {
            "type": ["string", "null"],
            "description": "ISO 8601"
          },
          "duration": {
            "type": ["number", "null"],
            "description": "Seconds."
          },
          "logical_date": {
            "type": ["string", "null"],
            "description": "ISO 8601"
          },
          "run_after": {
            "type": "string",
            "description": "ISO 8601"
          },
          "queued_when": {
            "type": ["string", "null"],
            "description": "ISO 8601"
          },
          "scheduled_when": {
            "type": ["string", "null"],
            "description": "ISO 8601"
          },
          "hostname": {
            "type": ["string", "null"]
          },
          "pool": {
            "type": "string"
          },
          "queue": {
            "type": ["string", "null"]
          },
          "executor": {
            "type": ["string", "null"]
          },
          "note": {
            "type": ["string", "null"]
          },
          "rendered_map_index": {
            "type": ["string", "null"]
          }
        },
        "required": [
          "id",
          "task_id",
          "dag_id",
          "dag_run_id",
          "map_index",
          "state",
          "try_number",
          "max_tries"
        ]
      }
    },
    "total_entries": {
      "type": ["integer", "null"],
      "description": "Populated for offset pagination; null for cursor pagination."
    },
    "next_cursor": {
      "type": ["string", "null"],
      "description": "Populated for cursor pagination; null otherwise or on the last page."
    },
    "previous_cursor": {
      "type": ["string", "null"],
      "description": "null on the first page."
    }
  },
  "required": [
    "task_instances"
  ]
}
```

### 4.3.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def taskinstance_list(cfg, promise):
    api = f"{cfg.base_url}/api/v2"
    auth = _token(cfg)
    args = promise.param["args"]

    # Pagination is the caller's loop (one promise per page), not ours.
    params = {k: v for k, v in args.items() if k not in ("dag_id", "dag_run_id")}
    dag_id = quote(args["dag_id"], safe="")
    dag_run_id = quote(args["dag_run_id"], safe="")
    r = requests.get(
        f"{api}/dags/{dag_id}/dagRuns/{dag_run_id}/taskInstances",
        headers=auth,
        params=params,
        timeout=10,
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found"})
    if r.status_code in (400, 422):
        return ("rejected", {"code": "invalid_request", "detail": r.json()["detail"]})
    _check(r)
    return ("resolved", r.json())
```


### 4.4 xcom.get

| | |
|---|---|
| **Documentation** | [Get Xcom Entry](https://airflow.apache.org/docs/apache-airflow/stable/stable-rest-api-ref.html#/XCom/get_xcom_entry) |

```json
{ "func": "xcom.get", "args": { ... } }
```

### 4.4.1 Promise Param Schema

```json
{
  "description": "Read one XCom entry produced by a task instance — the result value a Dag run published. Rejects not_found if the Dag, run, task, or key does not match an entry, invalid_request on a malformed request. Values are read straight from the metadata database, bypassing any custom XCom backend.",
  "type": "object",
  "properties": {
    "dag_id": {
      "type": "string"
    },
    "dag_run_id": {
      "type": "string",
      "description": "Run id."
    },
    "task_id": {
      "type": "string",
      "description": "Task that pushed the entry."
    },
    "xcom_key": {
      "type": "string",
      "description": "XCom key; a task's return value is stored under \"return_value\"."
    },
    "map_index": {
      "type": "integer",
      "minimum": -1,
      "description": "Map index of a dynamically mapped task; -1 when omitted."
    },
    "deserialize": {
      "type": "boolean",
      "description": "Render the stored value into a human-readable form. false when omitted."
    },
    "stringify": {
      "type": "boolean",
      "description": "Return value as a string instead of native JSON. false when omitted."
    }
  },
  "required": [
    "dag_id",
    "dag_run_id",
    "task_id",
    "xcom_key"
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
      "description": "invalid_request: = response.body.detail of the 400/422; not_found: absent"
    }
  },
  "required": ["code"]
}
```

### 4.4.3 Integration Request

```
GET /api/v2/dags/{promise.param.dag_id}/dagRuns/{promise.param.dag_run_id}/taskInstances/{promise.param.task_id}/xcomEntries/{promise.param.xcom_key}{?map_index,deserialize,stringify = promise.param.*} → 200
Authorization: Bearer {access_token}
```

### 4.4.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "key": {
      "type": "string"
    },
    "value": {
      "description": "The XCom value: native JSON, or a string when stringify is set."
    },
    "timestamp": {
      "type": "string",
      "description": "ISO 8601"
    },
    "logical_date": {
      "type": ["string", "null"],
      "description": "ISO 8601"
    },
    "run_after": {
      "type": "string",
      "description": "ISO 8601"
    },
    "map_index": {
      "type": "integer"
    },
    "task_id": {
      "type": "string"
    },
    "task_display_name": {
      "type": "string"
    },
    "dag_id": {
      "type": "string"
    },
    "dag_display_name": {
      "type": "string"
    },
    "run_id": {
      "type": "string"
    }
  },
  "required": [
    "key",
    "value",
    "timestamp",
    "map_index",
    "task_id",
    "dag_id",
    "run_id"
  ]
}
```

### 4.4.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def xcom_get(cfg, promise):
    api = f"{cfg.base_url}/api/v2"
    auth = _token(cfg)
    args = promise.param["args"]

    keys = ("map_index", "deserialize", "stringify")
    params = {k: args[k] for k in keys if k in args}
    dag_id, dag_run_id, task_id, xcom_key = (
        quote(args[k], safe="") for k in ("dag_id", "dag_run_id", "task_id", "xcom_key")
    )
    r = requests.get(
        f"{api}/dags/{dag_id}/dagRuns/{dag_run_id}"
        f"/taskInstances/{task_id}/xcomEntries/{xcom_key}",
        headers=auth,
        params=params,
        timeout=10,
    )
    if r.status_code == 404:
        return ("rejected", {"code": "not_found"})
    if r.status_code in (400, 422):
        return ("rejected", {"code": "invalid_request", "detail": r.json()["detail"]})
    _check(r)
    return ("resolved", r.json())
```


### 4.5 dag.get

| | |
|---|---|
| **Documentation** | [Get Dag Details](https://airflow.apache.org/docs/apache-airflow/stable/stable-rest-api-ref.html#/DAG/get_dag_details) |

```json
{ "func": "dag.get", "args": { ... } }
```

### 4.5.1 Promise Param Schema

```json
{
  "description": "Fetch one Dag with its full details, including \"params\" — the declared parameters and their defaults, which are the valid keys of dagrun.trigger's conf for this Dag. Rejects not_found if the dag_id is unknown, invalid_request on a malformed request.",
  "type": "object",
  "properties": {
    "dag_id": {
      "type": "string"
    }
  },
  "required": [
    "dag_id"
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
      "description": "invalid_request: = response.body.detail of the 400/422; not_found: absent"
    }
  },
  "required": ["code"]
}
```

### 4.5.3 Integration Request

```
GET /api/v2/dags/{promise.param.dag_id}/details → 200
Authorization: Bearer {access_token}
```

### 4.5.4 Integration Response

```json
{
  "type": "object",
  "description": "Every property of a 4.6.4 dags[] item, plus the details-only properties below.",
  "properties": {
    "concurrency": {
      "type": "integer"
    },
    "params": {
      "type": ["object", "null"],
      "description": "Declared Dag parameters and their defaults; the valid conf keys for this Dag.",
      "additionalProperties": true
    },
    "doc_md": {
      "type": ["string", "null"]
    },
    "catchup": {
      "type": "boolean"
    },
    "dag_run_timeout": {
      "type": ["string", "null"],
      "description": "ISO 8601 duration; the run is failed by Airflow once exceeded."
    },
    "start_date": {
      "type": ["string", "null"],
      "description": "ISO 8601"
    },
    "end_date": {
      "type": ["string", "null"],
      "description": "ISO 8601"
    },
    "timezone": {
      "type": ["string", "null"]
    },
    "active_runs_count": {
      "type": "integer"
    },
    "default_args": {
      "type": ["object", "null"],
      "additionalProperties": true
    },
    "asset_expression": {
      "type": ["object", "null"],
      "additionalProperties": true
    },
    "template_search_path": {
      "type": ["array", "null"],
      "items": {
        "type": "string"
      }
    },
    "render_template_as_native_obj": {
      "type": "boolean"
    },
    "is_paused_upon_creation": {
      "type": ["boolean", "null"]
    },
    "rerun_with_latest_version": {
      "type": ["boolean", "null"]
    },
    "owner_links": {
      "type": ["object", "null"],
      "additionalProperties": true
    },
    "latest_dag_version": {
      "type": ["object", "null"]
    },
    "last_parsed": {
      "type": ["string", "null"],
      "description": "ISO 8601"
    },
    "is_favorite": {
      "type": "boolean"
    }
  },
  "required": [
    "dag_id"
  ]
}
```

### 4.5.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def dag_get(cfg, promise):
    api = f"{cfg.base_url}/api/v2"
    auth = _token(cfg)
    dag_id = quote(promise.param["args"]["dag_id"], safe="")

    r = requests.get(f"{api}/dags/{dag_id}/details", headers=auth, timeout=10)
    if r.status_code == 404:
        return ("rejected", {"code": "not_found"})
    if r.status_code in (400, 422):
        return ("rejected", {"code": "invalid_request", "detail": r.json()["detail"]})
    _check(r)
    return ("resolved", r.json())
```


### 4.6 dag.list

| | |
|---|---|
| **Documentation** | [Get Dags](https://airflow.apache.org/docs/apache-airflow/stable/stable-rest-api-ref.html#/DAG/get_dags) |

```json
{ "func": "dag.list", "args": { ... } }
```

### 4.6.1 Promise Param Schema

```json
{
  "description": "List the Dags known to the scheduler, for discovering dag_ids and composing dagrun.trigger calls. Rejects invalid_request on a malformed filter.",
  "type": "object",
  "properties": {
    "dag_id_pattern": {
      "type": "string",
      "description": "Case-insensitive substring match (SQL ILIKE); \"|\" means OR, \"~\" matches everything. Not index-friendly on large tables."
    },
    "dag_id_prefix_pattern": {
      "type": "string",
      "description": "Case-sensitive prefix match; index-friendly, prefer at scale."
    },
    "tags": {
      "type": "array",
      "items": {
        "type": "string"
      }
    },
    "tags_match_mode": {
      "type": "string",
      "enum": ["any", "all"]
    },
    "owners": {
      "type": "array",
      "items": {
        "type": "string"
      }
    },
    "paused": {
      "type": "boolean",
      "description": "Restrict to paused or unpaused Dags."
    },
    "exclude_stale": {
      "type": "boolean",
      "description": "Exclude Dags no longer present in the bundle; true when omitted."
    },
    "has_import_errors": {
      "type": "boolean"
    },
    "last_dag_run_state": {
      "type": "string",
      "enum": ["queued", "running", "success", "failed"]
    },
    "bundle_name": {
      "type": "string"
    },
    "limit": {
      "type": "integer",
      "minimum": 0,
      "description": "Page size; 50 when omitted."
    },
    "offset": {
      "type": "integer",
      "minimum": 0,
      "description": "0 when omitted."
    },
    "order_by": {
      "type": "array",
      "description": "Sort attributes; prefix with \"-\" for descending. Supported: dag_id, dag_display_name, next_dagrun, state, start_date, last_run_state, last_run_start_date. [\"dag_id\"] when omitted.",
      "items": {
        "type": "string"
      }
    }
  }
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

```json
{
  "type": "object",
  "properties": {
    "code": {
      "type": "string",
      "enum": ["invalid_request"]
    },
    "detail": {
      "description": "= response.body.detail of the 400/422"
    }
  },
  "required": ["code"]
}
```

### 4.6.3 Integration Request

```
GET /api/v2/dags{?dag_id_pattern,dag_id_prefix_pattern,tags,tags_match_mode,owners,paused,exclude_stale,has_import_errors,last_dag_run_state,bundle_name,limit,offset,order_by = promise.param.*} → 200
Authorization: Bearer {access_token}
```

### 4.6.4 Integration Response

```json
{
  "type": "object",
  "properties": {
    "dags": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "dag_id": {
            "type": "string",
            "description": "External identity."
          },
          "dag_display_name": {
            "type": "string"
          },
          "is_paused": {
            "type": "boolean",
            "description": "A paused Dag accepts triggers but its runs stay queued."
          },
          "is_stale": {
            "type": "boolean",
            "description": "No longer present in its bundle; a stale Dag cannot be triggered."
          },
          "has_import_errors": {
            "type": "boolean",
            "description": "A Dag with import errors cannot be triggered."
          },
          "allowed_run_types": {
            "type": ["array", "null"],
            "description": "Run types this Dag permits; manual triggers fail unless \"manual\" is present or the value is null.",
            "items": {
              "type": "string",
              "enum": ["backfill", "scheduled", "manual", "operator_triggered", "asset_triggered", "asset_materialization"]
            }
          },
          "description": {
            "type": ["string", "null"]
          },
          "owners": {
            "type": "array",
            "items": {
              "type": "string"
            }
          },
          "tags": {
            "type": "array",
            "items": {
              "type": "object",
              "properties": {
                "name": {
                  "type": "string"
                },
                "dag_id": {
                  "type": "string"
                }
              }
            }
          },
          "timetable_summary": {
            "type": ["string", "null"]
          },
          "timetable_description": {
            "type": ["string", "null"]
          },
          "timetable_partitioned": {
            "type": "boolean"
          },
          "timetable_periodic": {
            "type": "boolean"
          },
          "next_dagrun_logical_date": {
            "type": ["string", "null"],
            "description": "ISO 8601"
          },
          "next_dagrun_run_after": {
            "type": ["string", "null"],
            "description": "ISO 8601"
          },
          "next_dagrun_data_interval_start": {
            "type": ["string", "null"],
            "description": "ISO 8601"
          },
          "next_dagrun_data_interval_end": {
            "type": ["string", "null"],
            "description": "ISO 8601"
          },
          "max_active_runs": {
            "type": ["integer", "null"]
          },
          "max_active_tasks": {
            "type": "integer"
          },
          "max_consecutive_failed_dag_runs": {
            "type": "integer"
          },
          "has_task_concurrency_limits": {
            "type": "boolean"
          },
          "is_backfillable": {
            "type": "boolean"
          },
          "bundle_name": {
            "type": ["string", "null"]
          },
          "bundle_version": {
            "type": ["string", "null"]
          },
          "fileloc": {
            "type": "string"
          },
          "relative_fileloc": {
            "type": ["string", "null"]
          },
          "file_token": {
            "type": "string"
          },
          "last_parsed_time": {
            "type": ["string", "null"],
            "description": "ISO 8601"
          },
          "last_parse_duration": {
            "type": ["number", "null"],
            "description": "Seconds."
          },
          "last_expired": {
            "type": ["string", "null"],
            "description": "ISO 8601"
          }
        },
        "required": [
          "dag_id",
          "dag_display_name",
          "is_paused",
          "is_stale",
          "has_import_errors",
          "allowed_run_types",
          "owners",
          "tags",
          "fileloc"
        ]
      }
    },
    "total_entries": {
      "type": "integer"
    }
  },
  "required": [
    "dags",
    "total_entries"
  ]
}
```

### 4.6.5 Implementation

| | |
|---|---|
| **Invocation** | `read` |
| **Monitoring** | `request_response` |

```python
def dag_list(cfg, promise):
    api = f"{cfg.base_url}/api/v2"
    auth = _token(cfg)
    args = promise.param["args"]

    # Pagination is the caller's loop (one promise per page), not ours.
    r = requests.get(f"{api}/dags", headers=auth, params=args, timeout=10)
    if r.status_code in (400, 422):
        return ("rejected", {"code": "invalid_request", "detail": r.json()["detail"]})
    _check(r)
    return ("resolved", r.json())
```


## 5. Test

### 5.1 Base Image

```dockerfile
FROM apache/airflow:3.3.1
ENV AIRFLOW__CORE__LOAD_EXAMPLES=True
CMD ["standalone"]
```

### 5.2 Run

```sh
docker rm -f plugin-airflow-test >/dev/null 2>&1 || true
docker build -t plugin-airflow-test spec/
docker run -d --name plugin-airflow-test -p 8080:8080 plugin-airflow-test

until AIRFLOW_PASSWORD=$(docker exec plugin-airflow-test python3 -c "import json; print(json.load(open('/opt/airflow/simple_auth_manager_passwords.json.generated'))['admin'])" 2>/dev/null) && [ -n "$AIRFLOW_PASSWORD" ]; do sleep 5; done
until curl -sf http://localhost:8080/api/v2/version >/dev/null; do sleep 5; done
until curl -sf -X POST http://localhost:8080/auth/token -H "Content-Type: application/json" -d "{\"username\": \"admin\", \"password\": \"$AIRFLOW_PASSWORD\"}" >/dev/null; do sleep 5; done

TOKEN=$(curl -sf -X POST http://localhost:8080/auth/token -H "Content-Type: application/json" -d "{\"username\": \"admin\", \"password\": \"$AIRFLOW_PASSWORD\"}" | python3 -c "import json,sys; print(json.load(sys.stdin)['access_token'])")
for dag in example_simplest_dag example_failed_dag example_xcom; do
  curl -sf -X PATCH "http://localhost:8080/api/v2/dags/$dag?update_mask=is_paused" -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" -d '{"is_paused": false}' >/dev/null
done

export AIRFLOW_BASE_URL=http://localhost:8080
export AIRFLOW_FIXTURE_OK=example_simplest_dag
export AIRFLOW_FIXTURE_FAIL=example_failed_dag
export AIRFLOW_USERNAME=admin
export AIRFLOW_PASSWORD
```
